---
title: "Kask Panel Redesign — Multi-Tab Per-Server Curator Threads"
audience: [zed-kask integrators, hKask architects, GPUI engineers]
last_updated 2026-07-28
version: "0.3.0"
status: "Draft"
domain: "composition"
mds_categories: [composition, trust, lifecycle, curation]
---

# Kask Panel Redesign — Multi-Tab Per-Server Curator Threads

> **One-line frame:** The kask panel is a center-pane `Item` with a row of
> **tabs at the top** (one per built-in kask MCP server) and a single
> conversation surface below. Each tab is an **independent curator thread**
> with its own history, its own tool scope (the tab's MCP server), and its
> own per-tab system prompt. The curator observes MCP usage **across tabs**
> because the curator MCP server already owns `EpisodicMemory`,
> `SemanticMemory`, and a `RegulationArchive`, and `McpRuntime` already
> records every governed tool invocation's outcome in the
> `RegulationLedger` — no panel-side event forwarding. The panel's job is
> **input + display**; cross-tab curation is the curator server's job.

---

## 0. v2 → v3 correction: the fork was gold-plating

The v2 plan proposed forking ~29,000 lines of `agent_ui` code
(`ConversationView` 11k + `ThreadView` 12.6k + `MessageEditor` 5.7k) into
`kask_panel`, deleting ~40% (ACP, auth, elicitation, terminal, subagents,
model selector, profiles), and keeping the rest for "pixel-parity with the
agent panel." Three lenses kill that design:

### Essentialist (Exist → Surface → Contract)

- **Exist (deletion test):** Delete the fork. Does complexity reappear
  elsewhere? No — it reappears *inside the agent panel*, which already has
  `ConversationView` parameterized by `Agent` with `Agent::Curator` as a
  first-class option, and `retained_threads: HashMap<ThreadId,
  Entity<ConversationView>>` for multiple independent threads. The fork
  duplicates an abstraction that already exists and is parameterized for
  exactly this use case. The fork does not deserve to exist.
- **Surface:** The user's directive was *"each tab is a context focused
  **mini**-replica of the agent panel."* v2 read "mini-replica" as "full
  replica, minus the parts I deleted." That's maximal-with-cuts, not
  minimal. A genuine mini-replica asks: what's the smallest surface that
  satisfies "per-server curator conversation"? Answer: input + display with
  markdown + streaming + tool cards. The current 1,608-line panel already
  does input + display (poorly). The gap is hundreds of lines, not tens of
  thousands.
- **Contract:** The one genuinely new abstraction — `CuratorSession`
  (per-tab, stateful, streaming, tool-scoped, replacing the stateless
  `ScopedInference`) — is ~150 lines of trait + enum. It does not require
  the fork to be useful. It requires a `Vec<ChatMessage>` history, a
  streaming call to `generate_stream_with_messages`, and tool dispatch.

### Pragmatic-cybernetics

- **Ashby's Law (requisite variety):** The panel regulates the user's
  interaction with 10 MCP servers via the curator. Requisite variety: 10
  servers, per-server history, per-server tool scope, streaming text,
  tool-call results. The v2 fork added variety for ACP negotiation,
  agent-server auth, elicitation forms, terminal codegen, subagent
  navigation, profile/mode/thinking-effort menus, model selector, trial
  upsell, onboarding, buffer search. **None of that variety maps to the
  panel's regulatory task.** The plan even deleted it — confirming it was
  never requisite. Excess variety copied because it was in the source file.
- **Good Regulator theorem:** A regulator must model the system it
  regulates. The panel regulates *the curator + 10 MCP servers*. v2 made
  the panel a model of *the agent panel*. Those are different systems —
  which is why v2 had to delete 40%: the model didn't fit the system, so it
  carved off the mismatched parts. A purpose-built model (current panel +
  markdown + streaming) is a better regulator because it models the actual
  system.
- **The feedback-loop point (v2 §1.4 is the strongest argument *against*
  the fork):** Cross-tab observation lives in the curator server's
  `EpisodicMemory`/`SemanticMemory` + `RegulationLedger`. The panel
  forwards nothing between threads. So the panel's conversation surface is
  **stateless with respect to cross-tab curation** — it's 10 independent
  input/output channels. That's exactly what the current panel already is,
  minus rendering quality. The fork buys rendering quality at the cost of a
  parallel 29k-line regulator the cybernetic loop doesn't use.

### Coding-guidelines (Karpathy)

1. **Think before coding** — v2 skipped "do we need a fork at all" and
   jumped to "how do we fork."
2. **Simplicity first** — 29k-line fork is not the simplest solution to
   "per-server curator tabs with nice rendering."
3. **Surgical changes** — forking 3 whole files and deleting 40% is the
   opposite of surgical. Adding markdown + streaming to the existing panel
   is surgical.
4. **Goal-driven** — the goal is "context-focused mini-replica." v2
   delivered "full replica with amputations."

### The alternative v2 considered and rejected too quickly

v2 §1.5 asserts "the kask panel stays a center-pane `Item`" and uses that
to justify the fork (the agent panel is a dock `Panel`, so its
conversation surface can't be reused). But the center-pane-vs-dock
decision is **orthogonal** to the conversation-surface decision. Two
options v2 didn't weigh:

- **(A) Reuse the agent panel directly.** Deploy 10 curator-bound threads
  in the agent panel, one per MCP server, with per-thread tool scope. The
  agent panel already supports `Agent::Curator` and `retained_threads`.
  Cost: a per-thread tool-scope mechanism in agent_ui (small). No fork, no
  new panel. The kask panel `Item` becomes unnecessary for the
  conversation use case — it survives only for the kanban/portfolio/
  scenarios views.
- **(B) Keep the kask panel `Item`, grow it minimally.** Add markdown
  rendering, streaming, tool-call cards, and per-tab history to the
  existing 1,608-line panel. No fork. The panel stays a purpose-built
  regulator of MCP servers, not a model of the agent panel.

**v3 chooses (B).** Rationale: the kask panel's center-pane `Item` hosting
is a real product decision (it lives next to the editor/terminal, not in
the dock), and the per-tab tool-scope + per-tab system-prompt framing is
specific to the kask MCP-server domain and doesn't belong in agent_ui.
But the *rendering surface* is grown in place, not forked. The panel
remains a mini-replica: it borrows the `markdown` crate and the
`render_agent_markdown` *helper function* (a few dozen lines, lifted or
re-exported), not the 12k-line `ThreadView` that contains it.

---

## 1. Research Findings (verified against the codebase)

### 1.1 The current panel and what it lacks

`crates/kask_panel/src/kask_panel.rs` (1,608 lines) is a center-pane `Item`
with: a server-selector button row, a flat `div().children()` message list
rendered as `Label`s, a bare `Editor` input with a `KaskToolCompletionProvider`
for `/tool_name` completion, a status bar (gas gauge + regulation health),
and in-memory `HashMap<usize, Vec<KaskMessage>>` history per server.

What it does well: per-server history, direct `/tool_name` invocation,
scoped inference, status bar, slash commands (`/help`, `/clear`, `/tools`).

What it lacks (the actual gap, not 29k lines):
- **Markdown rendering** — messages are `Label::new(format!("{prefix}{}",
  msg.content))`. The `markdown` crate (`MarkdownElement` + `MarkdownStyle`)
  is already a workspace dependency path; agent_ui uses it directly.
- **Streaming** — `ScopedInference::infer` returns a single `String`. The
  `InferencePort::generate_stream_with_messages` API (§1.3) already
  streams `InferenceStreamChunk`s. The bridge adapter (`PanelScopedInference`
  in `main.rs:2226`) just doesn't call it.
- **Tool-call cards** — tool results render as `format!("{tool}\n{formatted}")`
  in a `Label`. A card (tool name, status, collapsible raw input, output,
  copy) is a few hundred lines, not a 12k-line fork.
- **Per-tab stateful session** — `ScopedInference::infer` rebuilds
  `[system, user]` every call with no history. The actual defect. Fixed by
  `CuratorSession` (§2.1).
- **Tab strip** — the server selector is a wrapping button row. A proper
  `ui::Tab` strip is a small component.

### 1.2 Streaming is already supported by `InferencePort`

`hkask_types::InferencePort::generate_stream_with_messages`
(`kask/crates/hkask-types/src/ports/inference_port.rs:78-89`) returns
`Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send + '_>>`.

`InferenceStreamChunk` (`ports/inference_port.rs:186-195`):
```rust
pub struct InferenceStreamChunk {
    pub text_delta: String,
    pub reasoning_delta: String,   // thinking-mode reasoning
    pub model: String,
    pub finish_reason: Option<String>,
    pub usage: Option<InferenceUsage>,
    pub tool_calls: Vec<StructuredToolCall>,
}
```

Streaming, thinking deltas, and structured tool calls are all supported.
The `CuratorEvent` enum (§2.1) mirrors these fields directly. The bridge
adapter just needs to call `generate_stream_with_messages` instead of
`generate_with_messages`.

### 1.3 The curator observes across tabs via its own storage (verified)

The curator MCP server (`kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:31-37`)
holds `EpisodicMemory`, `SemanticMemory`, `RegulationArchive`, and a
`TokenRegistry`. It is a single process; every tab's curator thread talks
to the same server process and recalls from the same memory regardless of
which tab asked. `McpRuntime` records every governed tool invocation's
outcome in the `RegulationLedger` (`hkask-regulation/src/cybernetics_loop.rs:529`).
**No panel-side `observe_tool_use` forwarding layer is needed.** This is
the cybernetic proof that the panel's conversation surface can be
stateless w.r.t. cross-tab curation.

### 1.4 The center-pane `Item` stays

`KaskPanel` implements `workspace::item::Item` (center pane, multi-instance,
`Toggle` deploys). This matches `TerminalView` and is correct. The panel
is **not** converted to a dock `Panel`. The conversation surface is grown
in place inside the `Item`, not forked from the agent panel's dock.

### 1.5 The `markdown` crate is the rendering reuse, not `ThreadView`

`crates/markdown/src/markdown.rs` provides `Markdown`, `MarkdownElement`,
`MarkdownStyle`. `MarkdownStyle::themed(MarkdownFont::Agent, window, cx)`
is the agent-panel font/style. The agent panel's `render_agent_markdown`
(`agent_ui/src/conversation_view.rs:3386`) is a ~30-line helper that wraps
`MarkdownElement::new(markdown, style)` with a code-span resolver and
image/url click handlers. **This helper is the reuse unit** — lifted into
`kask_panel` or re-exported from `agent_ui` — not the 12k-line `ThreadView`
that contains it. The kask panel does not need `ListState` virtualization
(conversations are short), `MessageEditor` with mentions/queue/expand
(the bare `Editor` + completion provider suffices for v1), or
ACP/auth/elicitation/terminal/subagent code (deleted in v2, still deleted
in v3 — but v3 never copies it in the first place).

---

## 2. Design Decisions (pinned)

### 2.1 `CuratorSession` — the one genuinely new abstraction

The current `ScopedInference::infer(server, prompt, system_prompt)` is
stateless — the bridge builds a fresh `[system, user]` array each call.
The redesign introduces a `CuratorSession` trait, **one instance per tab**,
holding that tab's conversation history:

```rust
pub trait CuratorSession: Send + Sync {
    /// Send a user message to the curator with this tab's tool scope and
    /// system prompt. Returns a stream of curator events.
    fn send(
        &self,
        message: &str,
        tool_scope: &ToolScope,
        system_prompt: &str,
    ) -> Task<Result<CuratorEventStream, String>>;

    /// Cancel the in-flight curator turn for this tab.
    fn cancel(&self) -> Task<Result<()>>;

    /// Retry the last user message (re-send with the same scope + prompt).
    fn retry(&self) -> Task<Result<CuratorEventStream, String>>;
}

pub enum ToolScope {
    /// Only the named MCP server's tools are available to the curator.
    Server(String),
}

pub enum CuratorEvent {
    TextDelta(String),
    ThinkingDelta(String),
    ToolCall(StructuredToolCall),
    ToolResult { call_id: String, result: Value },
    Done { finish_reason: Option<String>, usage: Option<InferenceUsage> },
    Error(String),
}
```

The bridge (`main.rs`) provides `PanelCuratorSession` (one per tab),
wrapping the `InferencePort` (via `generate_stream_with_messages`), the
`ToolPort` (OCAP-gated, reusing `PanelToolInvoker`'s `DelegationToken`
machinery), and a `tokio::sync::Mutex<Vec<ChatMessage>>` for that tab's
history. A `set_curator_session_factory` OnceLock replaces
`set_scoped_inference`; the factory is called per-tab to construct a fresh
`PanelCuratorSession`. Wired in the deferred task in `main.rs` (per the
`.rules` "Model-dependent kask wiring must run in the deferred task" trap),
with a `log::warn!` in the failure branch naming the hook (per the
"process-global hooks need a startup-failure signal" trap).

**No `observe_tool_use` method.** Cross-tab observation is the curator
server's job (§1.3).

### 2.2 Per-tab threads — `HashMap<ServerId, TabState>`

The `KaskPanel` struct holds one `TabState` per MCP server, keyed by server
index — the structural mirror of the agent panel's `retained_threads`,
but **without forking `ConversationView`**. `TabState` is a small struct
holding the per-tab `CuratorSession`, history, and live-stream state:

```rust
struct TabState {
    /// This tab's curator session (own history mutex inside).
    session: Arc<dyn CuratorSession>,
    /// In-memory message history for rendering (mirrors the session's
    /// authoritative history; updated as events arrive).
    messages: Vec<KaskMessage>,
    /// The currently-streaming assistant message, if a turn is in flight.
    streaming: Option<StreamingMessage>,
    /// Whether a turn is in progress (drives spinner + cancel button).
    busy: bool,
}

struct StreamingMessage {
    /// Accumulated text deltas for the live markdown render.
    text: String,
    /// Accumulated thinking deltas (rendered as a collapsible block).
    thinking: String,
    /// Tool calls emitted so far this turn (rendered as cards).
    tool_calls: Vec<ToolCallEntry>,
}
```

This is ~50 lines of state, not a 12k-line `ThreadView`. The panel renders
the active tab's `TabState` directly; switching tabs swaps which
`TabState` is rendered.

### 2.3 The tab strip replaces `render_server_selector`

The current `render_server_selector` (L699–733) — a `v_flex` with a label,
a wrapping row of `Button`s, and a "Selected: {current}" label — becomes a
proper tab strip at the top of the panel. One tab per
`BUILT_IN_MCP_SERVERS_IDS` entry (10 tabs). The active tab is highlighted;
clicking switches `active_tab` and swaps the rendered `TabState`. Each tab
shows the server name. This is a small `render_tab_strip` method, not a
separate crate module.

### 2.4 Markdown rendering via the `markdown` crate

`render_messages` replaces `Label::new(format!("{prefix}{}", msg.content))`
with `MarkdownElement` from the `markdown` crate. Each assistant message
holds an `Entity<Markdown>`; streaming updates append to the markdown
source and call `cx.notify()`. The `render_agent_markdown` helper
(~30 lines) is lifted into `kask_panel` or re-exported from `agent_ui`
for code-span link + image + url-click handling. System/error messages
render as a `Callout`. This is the bulk of the rendering upgrade and it's
~200 lines, not 12k.

### 2.5 Tool-call cards — a small component, not a fork

Tool calls (curator-emitted and direct `/tool_name` invocations) render as
cards: tool name, status (pending/running/done/error), collapsible raw
input, output (markdown or raw), copy button. This is a
`render_tool_call_card` function (~150 lines) in `kask_panel`, not a port
of `ThreadView::render_tool_call` (which carries ACP permission prompts,
session-id lookup, and subagent machinery). The kask panel has no
permission prompts (OCAP tokens are pre-authorized by the bridge) and no
subagents — the card is simpler than its agent-panel counterpart.

### 2.6 System-prompt template per tab (Jinja2)

The current `build_system_prompt` (L224–272) is a Rust `format!` string.
The redesign replaces it with a Jinja2 template matching the kask skill
cascade pattern. The template receives `{{ server }}`,
`{{ server_description }}`, `{{ tools }}` (list of `{name, description}`),
`{{ task }}` (the user's current request, per the `.rules` "Skill cascade
context must carry the user's task" trap), and `{{ curator_guidance }}`
(shared include appended to every tab's system prompt).

Templates live in `kask/registry/panel-prompts/`:
- `panel-tab-system.j2` — the per-tab framing (parameterized by server).
- `panel-curator-guidance.j2` — the shared curator guidance.

### 2.7 What is deliberately NOT done (the v2 cuts that stay cut)

These v2 "kept" items are **not** carried into v3, because they are excess
variety for the panel's regulatory task (§0, cybernetics lens):

- `ListState`-backed virtualization — kask conversations are short; a
  flat `div().children()` with a `ScrollHandle` suffices. Add virtualization
  only if a real conversation is observed to jank.
- `MessageEditor` fork (mentions, slash-commands menu, context chips,
  queue, expand-to-fullscreen) — the bare `Editor` + `KaskToolCompletionProvider`
  already handles `/tool_name` completion. `@`-mentions and a slash-command
  menu are v2 features, deferred until a user asks for them.
- `WithRemSize` + `agent_ui_font_size` + cmd-+/cmd- — not a panel
  requirement; the panel uses the default UI font.
- Drag-and-drop files — not a panel requirement.
- `ThreadStore` persistence — v1 keeps conversations in-memory (matches
  current behavior). v2 can add a `KaskThreadStore`.
- Retry / cancel / undo-last-reject — cancel is cheap and included (the
  `CuratorSession::cancel` method exists). Retry/undo are deferred.

---

## 3. Architecture

### 3.1 Crate structure — minimal growth

```
crates/kask_panel/src/
├── kask_panel.rs          # The Item shell: tab strip + active TabState render
├── curator_session.rs     # CuratorSession trait + ToolScope + CuratorEvent + factory
├── tool_call_card.rs      # render_tool_call_card (new, ~150 lines)
├── panel_button.rs        # (unchanged) status bar toggle button
├── kanban_view.rs         # (unchanged) separate center-pane Item
├── portfolio_view.rs      # (unchanged) separate center-pane Item
└── scenarios_view.rs      # (unchanged) separate center-pane Item
```

New `Cargo.toml` dependencies: `markdown`, `theme_settings`, `component`
(for `Callout` / `CopyButton` if not already via `ui`). No new crates.

**No `conversation_view.rs`, no `thread_view.rs`, no `message_editor.rs`
fork.** The panel grows by ~600 lines (curator_session + tool_call_card +
markdown rendering + tab strip), not ~29,000.

### 3.2 The `KaskPanel` shell

```rust
pub struct KaskPanel {
    _workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    /// The active tab (index into BUILT_IN_MCP_SERVERS_IDS).
    active_tab: usize,
    /// One TabState per MCP server — the structural mirror of the agent
    /// panel's `retained_threads`, keyed by server index. Each tab is an
    /// independent curator conversation scoped to that server's tools.
    tabs: HashMap<usize, TabState>,
    /// Latest regulation snapshot (for the status bar gas gauge).
    regulation_snapshot: RegulationSnapshot,
    status_fetching: bool,
    /// Scroll handle for the messages container.
    messages_scroll_handle: gpui::ScrollHandle,
    /// The message input editor (shared across tabs; cleared on send).
    input_editor: Entity<Editor>,
}
```

The `Item` impl is preserved (tab content text, serialization, focus). The
`Render` impl becomes:

```rust
v_flex()
    .size_full()
    .track_focus(&self.focus_handle)
    .child(self.render_tab_strip(cx))                 // ← new
    .child(self.render_messages(window, cx))           // ← markdown-upgraded
    .child(self.render_status_bar(cx))                 // ← unchanged
    .child(self.render_input(cx))                      // ← unchanged
```

### 3.3 The `CuratorSession` and the bridge

The `CuratorSession` trait (§2.1) is implemented in `main.rs` by a
`PanelCuratorSession` adapter, **one instance per tab**. It wraps the
`InferencePort` (via `generate_stream_with_messages`), the `ToolPort`
(OCAP-gated), and a `tokio::sync::Mutex<Vec<ChatMessage>>` for that tab's
history. The `send` method:
1. Appends the user message to this tab's history.
2. Prepends the per-tab system prompt as the leading `system` message.
3. Calls `InferencePort::generate_stream_with_messages` with the history +
   the tab's tool definitions.
4. Streams `InferenceStreamChunk`s back as `CuratorEvent::TextDelta` /
   `ThinkingDelta` / `ToolCall` / `Done`.
5. When the curator emits a `ToolCall`, the session calls `ToolPort::invoke`
   (OCAP-gated), emits `CuratorEvent::ToolResult`, and appends the tool
   call + result to the history.
6. Appends the final assistant response to the history.

A `set_curator_session_factory` OnceLock replaces `set_scoped_inference`.
The factory is called per-tab to construct a fresh `PanelCuratorSession`
with its own history mutex. Wired in the deferred task in `main.rs` with a
`log::warn!` in the failure branch naming the hook.

### 3.4 System-prompt placement (per turn, v1)

Send `[system(tab_prompt), ...history, user]` each turn. Simple,
model-agnostic. v2 can optimize to a single system message at session
start if the curator model supports mid-conversation system updates.

---

## 4. Migration Plan (phased, minimal)

### Phase 1: `CuratorSession` contract + factory (no behavior change)

1. Define `CuratorSession`, `ToolScope`, `CuratorEvent` in
   `kask_panel/src/curator_session.rs`.
2. Define `set_curator_session_factory` OnceLock (replaces
   `set_scoped_inference`).
3. Implement `PanelCuratorSession` in `main.rs` (wraps `InferencePort` +
   `ToolPort` + per-tab history mutex). Wire in the deferred task with the
   failure-branch `log::warn!`.
4. Keep the old `ScopedInference` path alive temporarily so the panel
   still works; the new factory is wired but not yet called by the panel.
5. **No visual change** — the panel still looks the same.

**Validation:** `cargo check -p kask_panel`, `cargo check -p zed`, existing
tests pass, panel opens and behaves as before.

### Phase 2: Tab strip + per-tab `TabState`

1. Replace `render_server_selector` with `render_tab_strip` (one tab per
   `BUILT_IN_MCP_SERVERS_IDS`, active tab highlighted, click switches).
2. Replace `conversations: HashMap<usize, Vec<KaskMessage>>` with
   `tabs: HashMap<usize, TabState>` (adds `session`, `streaming`, `busy`).
3. Switch `run_scoped_inference` to call the active tab's `CuratorSession::send`
   and consume the `CuratorEventStream`, populating `TabState::streaming`.
4. Direct `/tool_name` invocations still go through `ToolInvoker` (unchanged)
   and render as tool messages.

**Status: ✅ Tab strip done (Phase 2.1).** The `ui::Tab`-based strip replaces
the button row. Per-tab `TabState` consolidation is deferred — the existing
`HashMap<usize, Vec<KaskMessage>>` + `Option<Arc<dyn CuratorSession>>`
(reset on tab switch) already provides per-tab thread independence. The
`TabState` struct will be introduced when streaming state needs to persist
across tab switches (currently streaming is foreground-bound).

**Validation:** Clicking a tab switches the active conversation. Each tab
has its own independent history. Streaming text appears incrementally (as
a single concatenated string for now — markdown comes in Phase 3).

### Phase 3: Markdown rendering

1. Add `markdown`, `theme_settings`, `component` to `kask_panel/Cargo.toml`.
2. Lift `render_agent_markdown` (~30 lines) into `kask_panel` or re-export
   from `agent_ui`.
3. Replace `Label::new(format!("{prefix}{}", msg.content))` with
   `MarkdownElement` for assistant messages; system/error messages render
   as `Callout`.
4. Each assistant message holds an `Entity<Markdown>`; streaming updates
   append to the markdown source and call `cx.notify()`.

**Validation:** Send a message that returns markdown (headers, code blocks,
links, lists) — renders with syntax highlighting. Visual parity with the
agent panel for markdown content.

### Phase 4: Tool-call cards

1. Add `render_tool_call_card` in `kask_panel/src/tool_call_card.rs`
   (~150 lines): tool name, status, collapsible raw input, output
   (markdown or raw), copy button.
2. Wire `CuratorSession::send` to emit `CuratorEvent::ToolCall` /
   `ToolResult`; render them as cards in the message list.
3. Direct `/tool_name` invocations render as tool-call cards too.

**Status: ✅ Done.** `tool_call_card.rs` implements `ToolCallCard` (a
`Render` entity) with status icon, collapsible input, output, copy button.
Curator-emitted tool calls and direct `/tool_name` invocations both render
as cards.

**Validation:** Curator calls a tool → card appears with input/output. User
runs `/tool_name args` → card appears. Expand/collapse works. Copy works.

### Phase 5: System-prompt templates

1. Move `build_system_prompt` to a Jinja2 template in
   `kask/registry/panel-prompts/panel-tab-system.j2`.
2. Templates receive `{{ server }}`, `{{ tools }}`, `{{ task }}`,
   `{{ curator_guidance }}`.
3. The curator guidance include (`panel-curator-guidance.j2`) is appended
   to every tab's system prompt.

**Status: ✅ Done.** `system_prompt.rs` renders `panel-tab-system.j2` +
`panel-curator-guidance.j2` via `minijinja` (embedded at build time via
`include_str!`). The `build_system_prompt` function is now a thin wrapper.
7 unit tests cover template rendering.

### Phase 6: Cancel + tests

1. Wire `CuratorSession::cancel` to a cancel button in the status bar
   (shown when `busy`).
2. Tests pinning every deliberate deviation from the agent panel (per the
   `.rules` "tests must pin deliberate zed-kask deviations" trap): no
   `ListState` virtualization, no `MessageEditor` mentions/queue/expand, no
   `WithRemSize` font sizing, no drag-and-drop, no retry/undo.
3. Tests pinning the tab-strip behavior, per-tab thread independence, and
   that the curator's cross-tab recall flows through the curator server's
   memory (not through panel-side event forwarding).

**Status: ✅ Done.** Cancel button (IconButton with Stop icon) added to
the status bar, shown when `busy`. 13 deviation-pinning + behavior tests
added covering: no virtualization, no MessageEditor fork, no font sizing,
no drag-and-drop, no retry/undo, no ACP/auth/elicitation, no subagents,
no model selector, tab strip count, tab-switch session reset, and no
`observe_tool_use` method on `CuratorSession`.

---

## 5. Risks & Mitigations

| Risk | Mitigation |
|---|---|
| Lifting `render_agent_markdown` couples kask_panel to agent_ui internals. | The helper is ~30 lines and depends only on the `markdown` crate + a code-span resolver. If coupling is unwanted, copy the ~30 lines into kask_panel with a `// adapted from agent_ui` comment. |
| Per-tab `CuratorSession` instances each hold a conversation history — memory growth with 10 tabs. | Histories are bounded by the conversation length per tab; the curator server's episodic memory is the long-term store. v2 can add history truncation / condensation (the condenser MCP server exists for this). |
| The curator's cross-tab recall depends on the curator server's memory being populated. If the server is misconfigured (no `EpisodicMemory`), recall silently fails. | The curator server's `curator_ping` tool reports store availability. The panel can call it on first tab activation and warn if memory is unavailable (per the "process-global hooks need a startup-failure signal" trap). |
| Per-tab system prompts re-sent every turn is expensive. | Acceptable for v1 (§3.4). v2 optimizes. |
| Conversations lost on restart. | v1 matches current behavior. v2 adds `KaskThreadStore`. |
| Not forking means the panel doesn't get agent-panel features for free (mentions, model selector, profiles). | That's the point — those features are excess variety for the panel's regulatory task (§0). They can be added later if a user asks, as deliberate growth, not as inherited weight. |

---

## 6. What was wrong with v2 (recorded for the audit trail)

The v2 design doc proposed:

1. **Forking ~29,000 lines of `agent_ui`** (`ConversationView` + `ThreadView`
   + `MessageEditor`) into `kask_panel` and deleting ~40%. The essentialist
   deletion test fails: deleting the fork doesn't reintroduce complexity
   because the agent panel already has the abstraction, parameterized by
   `Agent` with `Agent::Curator` as a first-class option. The fork
   duplicates an existing regulator. The cybernetic lens confirms: the
   fork adds variety (ACP, auth, elicitation, terminal, subagents,
   profiles, model selector) that the panel's regulatory task doesn't
   require, then deletes it — confirming it was never requisite. The
   panel must model the curator + MCP servers, not the agent panel.
2. **Reading "mini-replica" as "full replica, minus cuts."** The user's
   directive was *"context focused **mini**-replica."* v2 delivered a
   maximal-with-cuts surface. A genuine mini-replica is the smallest
   surface that satisfies "per-server curator conversation" — input +
   display with markdown + streaming + tool cards, grown in place on the
   existing 1,608-line panel.
3. **Keeping `ListState` virtualization, `MessageEditor` with
   mentions/queue/expand, `WithRemSize` font sizing, drag-and-drop,
   retry/undo.** None of these map to the panel's regulatory task. They
   are excess variety copied because it was in the source file. v3
   defers all of them until a user asks, as deliberate growth.

v3 corrects this by growing the existing panel minimally (curator_session
contract + tab strip + markdown + tool cards, ~600 lines) instead of
forking a 29k-line regulator and amputating it. The one genuinely new
abstraction — `CuratorSession` (per-tab, stateful, streaming) — is kept;
it is the right contract. The fork is not.

---

## 7. Summary

The redesign is a **minimal in-place growth** of the existing `kask_panel`
crate, not a fork. The `KaskPanel` center-pane `Item` keeps its tab strip
at the top (one tab per built-in kask MCP server) and grows a per-tab
`TabState` (session + history + streaming state) — the structural mirror
of the agent panel's `retained_threads`, without copying its 29k-line
conversation surface. The one genuinely new abstraction is the per-tab
`CuratorSession` (stateful, streaming via
`InferencePort::generate_stream_with_messages`, tool-scoped). Rendering
upgrades (markdown via the `markdown` crate, tool-call cards) are
purpose-built ~200- and ~150-line components, not ports of `ThreadView`'s
12k-line surface. The curator observes across tabs because the curator
MCP server owns `EpisodicMemory` + `SemanticMemory` and `McpRuntime`
records every tool call's outcome in the `RegulationLedger` — no
panel-side event forwarding. The result is a context-focused mini-replica
of the agent panel's *conversation experience* (markdown, streaming, tool
cards), not a fork of the agent panel's *code*.
