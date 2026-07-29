---
title: "Kask Panel Redesign — Multi-Tab Per-Server Curator Threads"
audience: [zed-kask integrators, hKask architects, GPUI engineers]
last_updated 2026-07-28
version: "0.2.0"
status: "Draft"
domain: "composition"
mds_categories: [composition, trust, lifecycle, curation]
---

# Kask Panel Redesign — Multi-Tab Per-Server Curator Threads

> **One-line frame:** The kask panel is a center-pane `Item` that hosts a row
> of **tabs at the top** (one per built-in kask MCP server) and a stack of
> **threads** below it (one `ConversationView` per tab, forked from the agent
> panel's conversation surface). Each tab is a **context-focused mini-replica
> of the agent panel**: its own thread, its own conversation history, its own
> message editor, its own tool scope (the tab's MCP server), and its own
> per-tab system prompt that frames the curator's role in that server's
> domain. The **curator is the agent** the user talks to in every tab —
> exactly as the agent panel binds each thread to "Zed Agent" or "Curator"
> via the agent selector. The curator observes MCP usage patterns and tool
> use **across tabs** because the curator MCP server already owns
> `EpisodicMemory`, `SemanticMemory`, and a `RegulationArchive`, and the
> `McpRuntime` already records every governed tool invocation's outcome in
> the `RegulationLedger`. Beyond the tab strip and the curator-as-agent
> binding, the panel behaves and renders identically to the agent panel:
> rich markdown, streaming, tool-call cards, message editor with mentions
> and slash-commands, retry/cancel/copy, scroll, font-size, drag-and-drop.

---

## 0. The Abstraction I Missed (v1 → v2 correction)

The v1 design doc proposed a **single shared curator conversation** across
all tabs, with the tab switch only changing the "tool scope" passed to one
conversation. That was wrong. The user's directive — *"each tab is a context
focused mini-replica of the agent panel"* — maps directly onto the agent
panel's existing unit of conversation: **the thread**.

In the agent panel (`crates/agent_ui/src/agent_panel.rs:1165`):

```rust
draft_thread: Option<Entity<ConversationView>>,
retained_threads: HashMap<ThreadId, Entity<ConversationView>>,
```

Each `ConversationView` is one thread — one independent conversation bound to
one agent, with its own `ThreadView` (the 12k-line message list + editor +
tool cards), its own history, its own scroll position, its own retry/cancel
state. The agent panel's "tabs" (the thread list in the dock) are exactly
this: a map of `ThreadId → ConversationView`, with one active.

**The kask panel's tabs are the same abstraction.** Each tab is a
`ConversationView` (forked), bound to the curator agent, scoped to one MCP
server. The kask panel `Item` holds `HashMap<ServerId, Entity<KaskConversationView>>`
— the exact structural mirror of `retained_threads`, keyed by server
instead of by `ThreadId`. There is no shared conversation. Each tab is its
own thread.

The curator "observes across tabs" not because the panel forwards events
between threads (it doesn't — that would violate thread independence), but
because **the curator MCP server is a single process with its own persistent
memory**, and every tool call from every tab flows through the same
`McpRuntime`, which records outcomes in the same `RegulationLedger`, which
the curator server reads from. Cross-tab observation is a property of the
curator server's storage, not of the panel's UI state. This is verified in
§1.4.

---

## 1. Research Findings (verified against the codebase)

### 1.1 The agent panel's thread is the unit to fork

| Layer | File | Role |
|---|---|---|
| `AgentPanel` (dock shell) | `crates/agent_ui/src/agent_panel.rs:1153` | Dock `Panel`. Holds `draft_thread` + `retained_threads: HashMap<ThreadId, Entity<ConversationView>>`. **Not forked** — kask panel is a center-pane `Item`, not a dock. |
| `ConversationView` | `crates/agent_ui/src/conversation_view.rs:591` | One thread. Holds server state, auth, focus, code-span resolver, and a `ThreadView`. **Forked** → `KaskConversationView`. |
| `ThreadView` | `crates/agent_ui/src/conversation_view/thread_view.rs:565` | The conversation surface: `ListState`-backed message list, `render_entry`, `render_markdown`, `render_tool_call`, `render_thinking_block`, `render_generating`, `render_message_editor`, scroll actions, copy, retry, cancel. **Forked** → `KaskThreadView`. |
| `MessageEditor` | `crates/agent_ui/src/message_editor.rs:202` | Editor with mentions (`@`), slash commands (`/`), context chips, queue, expand, follow-up. **Forked** → `KaskMessageEditor`. |
| `render_agent_markdown` | `crates/agent_ui/src/conversation_view.rs:3386` | Shared markdown helper (`MarkdownElement` + code-block renderer + image resolver + url-click + code-span-link). **Reused as-is** (lifted into the kask_panel crate or re-exported). |
| `Markdown` / `MarkdownElement` / `MarkdownStyle` | `crates/markdown/src/markdown.rs` | The markdown crate. `MarkdownStyle::themed(MarkdownFont::Agent, window, cx)` is the agent-panel font/style. **Reused as-is.** |

### 1.2 What the agent panel does that the kask panel currently doesn't

| Capability | Agent panel | Current kask panel |
|---|---|---|
| Message rendering | `MarkdownElement` with syntax highlighting, copy buttons, wrap buttons, mermaid, images, link-click, code-span file links | `Label::new(format!("{prefix}{}", msg.content))` (L759) — plain text |
| Message list | `ListState`-backed virtualized list with `cx.processor` per entry | `div().children(message_elements)` — flat, not virtualized |
| Streaming | `AssistantMessageChunk::Message { block, .. }` with live `Entity<Markdown>` updates | Single `String` from `ScopedInference::infer` — no streaming |
| Tool calls | `render_tool_call` cards: expand/collapse, raw input, output markdown, error, copy, permission prompts | `format!("{tool}\n{formatted}")` in a `Label` (L604) |
| Thinking blocks | `AssistantMessageChunk::Thought` rendered collapsibly | None |
| Input editor | `MessageEditor` with mentions, slash commands, context chips, queue, expand, follow-up | Bare `Editor` + `KaskToolCompletionProvider` |
| Retry / cancel | `cancel_generation`, `retry`, `undo_last_reject` | None (busy flag only) |
| Copy | `CopyButton` on every message + `CopyThreadToClipboard` | None |
| Scroll | `ListState::scroll_to_end`, page-up/down, scroll-to-message | `ScrollHandle::scroll_to_bottom` |
| Persistence | `ThreadStore` + KVP serialization | `HashMap<usize, Vec<KaskMessage>>` in memory, lost on restart |
| Font size | `WithRemSize` + `agent_ui_font_size` + cmd-+/cmd- | None |
| Drag-and-drop files | `render_drag_target` + `ExternalPaths` | None |
| Thread independence | Each `ConversationView` is independent — own history, own scroll, own retry | N/A (single shared `Vec` per server) |

### 1.3 Streaming is already supported by `InferencePort`

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
The `CuratorEvent` enum mirrors `InferenceStreamChunk`'s fields directly.

### 1.4 The curator observes across tabs via its own storage (verified)

The curator MCP server (`kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs:31-37`)
holds:

```rust
pub struct CuratorServer {
    escalation_queue: Option<Arc<hkask_storage::EscalationQueue>>,
    regulation_store: Option<Arc<hkask_storage::RegulationArchive>>,
    episodic: Option<hkask_memory::EpisodicMemory>,        // ← episodic memory
    semantic: Option<Arc<hkask_memory::SemanticMemory>>,   // ← semantic memory
    token_registry: Option<Arc<dyn hkask_capability::TokenRegistry>>,
}
```

The curator server is a **single process** with its own `EpisodicMemory` and
`SemanticMemory`. Every tab's curator thread talks to the same server
process; the curator recalls from the same memory regardless of which tab
asked. This is the cross-tab observation mechanism — it lives in the
curator server, not in the panel.

Additionally, `McpRuntime` records every governed tool invocation's outcome
in the `RegulationLedger` (`hkask-regulation/src/cybernetics_loop.rs:529`:
"Record a tool outcome in the Regulation runtime for outcome quality
tracking. Called by McpRuntime after every governed tool invocation
completes."). So when the user (or the curator) calls a tool in any tab,
the outcome is recorded in the shared ledger, and the curator server can
read it. **No panel-side `observe_tool_use` forwarding layer is needed.**

### 1.5 The center-pane `Item` vs dock `Panel` distinction (unchanged)

`AgentPanel` implements `workspace::dock::Panel` (dock, singleton,
`ToggleFocus`). `KaskPanel` implements `workspace::item::Item` (center pane,
multi-instance, `Toggle` deploys). The kask panel **stays a center-pane
`Item`** — this matches `TerminalView` and is correct. The fork target is
the conversation surface (`ConversationView` + `ThreadView`), not the dock
shell (`AgentPanel`).

### 1.6 The agent selector is the curator binding

The agent panel's toolbar has an agent selector (`render_toolbar`,
`agent_panel.rs:5845`) listing `Agent::NativeAgent` (Zed Agent), `Agent::Curator`,
terminal, and custom agents. Each thread is bound to one agent via
`ConversationView`'s `connection_key: Agent` field (`conversation_view.rs:594`).

In the kask panel, **the curator is the only agent in the selector**. The
"selector" is implicit — every tab's thread is bound to the curator. The
tab strip replaces the agent selector: instead of choosing an agent, the
user chooses a **domain** (which MCP server's tools are in scope for this
curator thread). The agent is always the curator; the tab selects the tool
scope and the system-prompt framing.

---

## 2. Design Decisions (pinned)

### 2.1 Each tab is a thread — `HashMap<ServerId, Entity<KaskConversationView>>`

The `KaskPanel` struct holds one `KaskConversationView` per MCP server, keyed
by server ID — the structural mirror of the agent panel's
`retained_threads: HashMap<ThreadId, Entity<ConversationView>>`:

```rust
pub struct KaskPanel {
    _workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    /// The active tab (index into BUILT_IN_MCP_SERVERS_IDS).
    active_tab: usize,
    /// One thread per MCP server — the structural mirror of the agent
    /// panel's `retained_threads: HashMap<ThreadId, Entity<ConversationView>>`,
    /// keyed by server ID instead of ThreadId. Each tab is an independent
    /// curator conversation scoped to that server's tools and domain.
    threads: HashMap<usize, Entity<KaskConversationView>>,
    /// The tab strip state (ui::Tab components).
    tab_strip_state: TabStripState,
    /// Latest regulation snapshot (for the activity bar gas gauge).
    regulation_snapshot: RegulationSnapshot,
    status_fetching: bool,
}
```

Each `KaskConversationView` is independent: own history, own scroll, own
retry/cancel, own message editor, own tool scope. Switching tabs swaps which
`KaskConversationView` is rendered below the tab strip — exactly as the
agent panel swaps which `retained_thread` is rendered in the dock.

### 2.2 The fork is `ConversationView` + `ThreadView` + `MessageEditor`, simplified

The fork deletes ~40% of `ThreadView`:

**Deleted:**
- All ACP event handling (`AcpThreadEvent`, `ThreadStatus`, `SessionId`).
- Agent-server auth (`AuthState`, `auth_methods`, `reauthenticate`, `logout`).
- Elicitation (`render_request_elicitations`, `ElicitationFormState`).
- Terminal integration (`render_terminal`, `TerminalThreadMetadata`).
- Subagents (`render_subagent_titlebar`, `parent_session_id` navigation).
- Profiles / modes / thinking-effort / fast-mode menus.
- Model selector (`ModelSelectorPopover`) — the curator uses the configured
  default model; v2 can add a selector.
- Trial upsell / onboarding / codex-windows warning.
- Buffer search bar (`ThreadSearchBar`) — v1 skips in-thread search.
- Multi-root callout, external-source-prompt warning.

**Kept (the rendering surface):**
- `ListState`-backed message list with `render_entries`.
- `render_entry` → `render_markdown` (via `render_agent_markdown`).
- `render_tool_call` cards (expand/collapse, raw input, output markdown,
  error, copy).
- `render_thinking_block` (the curator model supports thinking via
  `reasoning_delta`).
- `render_generating` spinner.
- `render_message_editor` (forked to `KaskMessageEditor`).
- Scroll actions (page-up/down, to-top/bottom, to-message).
- Copy / retry / cancel actions.
- `render_thread_error` with `Callout`.
- `WithRemSize` font-size handling.
- Drag-and-drop file target (`render_drag_target`).

### 2.3 The tab strip replaces `render_server_selector`

The current `render_server_selector` (L699–733) — a `v_flex` with a label, a
wrapping row of `Button`s, and a "Selected: {current}" label — becomes a
proper tab strip at the top of the panel:

- One `ui::Tab` per `BUILT_IN_MCP_SERVERS_IDS` entry (10 tabs).
- The active tab is highlighted; clicking switches `active_tab` and swaps
  the rendered `KaskConversationView`.
- Each tab shows the server icon + name.
- The tab strip is part of the `KaskPanel` render (above the active
  `KaskConversationView`), not the workspace tab bar.
- This is the same `ui::Tab` component used in the agent panel's toolbar
  and settings pages — visual language parity.

### 2.4 The curator is the agent; the tab is the tool scope + system prompt

- Every `KaskConversationView` is bound to the **curator** agent. There is no
  agent selector — the curator is the only agent.
- Each tab's `KaskConversationView` is constructed with a **tool scope** (the
  tab's MCP server's tools) and a **system prompt** that frames the curator's
  role in that server's domain.
- The conversation history is **per-tab** (each tab is its own thread). The
  curator does not see other tabs' histories in its context window — it
  recalls across tabs via its own `EpisodicMemory` / `SemanticMemory` (§1.4),
  not via the panel concatenating threads.
- Direct `/tool_name args` invocation still works per-tab — it calls the
  active tab's MCP server directly, bypassing the curator, and the result is
  inserted into that tab's thread as a tool-call entry. The outcome is still
  recorded in the `RegulationLedger` by `McpRuntime`, so the curator can
  recall it.

### 2.5 System-prompt template per tab (Jinja2)

The current `build_system_prompt` (L224–272) is a Rust `format!` string. The
redesign replaces it with a **Jinja2 template** matching the kask skill
cascade pattern. The template receives `{{ server }}`,
`{{ server_description }}`, `{{ tools }}` (list of `{name, description}`),
`{{ task }}` (the user's current request, per the `.rules` "Skill cascade
context must carry the user's task" trap), and `{{ curator_guidance }}` (a
shared include appended to every tab's system prompt).

Templates live in `kask/registry/panel-prompts/`:
- `panel-tab-system.j2` — the per-tab framing (parameterized by server).
- `panel-curator-guidance.j2` — the shared curator guidance (remembering
  issues, observing tool-use patterns, cross-domain recall from episodic /
  semantic memory).

### 2.6 `ScopedInference` evolves to a per-thread `CuratorSession`

The current `ScopedInference::infer(server, prompt, system_prompt)` is
stateless — the bridge builds a fresh `[system, user]` array each call. The
redesign introduces a `CuratorSession` trait, **one instance per tab** (per
`KaskConversationView`), holding that tab's conversation history:

```rust
pub trait CuratorSession: Send + Sync {
    /// Send a user message to the curator with this tab's tool scope and
    /// system prompt. Returns a stream of curator events (text chunks,
    /// thinking deltas, tool calls, tool results, done).
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
wrapping:
- The `InferencePort` (via `generate_stream_with_messages`).
- The `ToolPort` (for the curator to call MCP tools — OCAP-gated, reusing
  `PanelToolInvoker`'s `DelegationToken` machinery).
- A `tokio::sync::Mutex<Vec<ChatMessage>>` for that tab's conversation
  history (persisted across `send` calls within the session).

**No `observe_tool_use` method.** Cross-tab observation is the curator
server's job (§1.4), not the panel's. The panel does not forward events
between threads — that would violate thread independence and duplicate the
curator server's own memory.

### 2.7 Streaming is required for parity — and the port supports it

The `KaskThreadView` consumes the `CuratorEventStream` incrementally,
updating the active `Entity<Markdown>` as `TextDelta` chunks arrive,
rendering `ThinkingDelta` as collapsible thinking blocks, and rendering
`ToolCall` / `ToolResult` as tool-call cards. This mirrors the agent panel's
`AssistantMessageChunk::Message { block, .. }` → live `Entity<Markdown>`
update pattern. `InferencePort::generate_stream_with_messages` (§1.3) makes
this possible without a new port method.

### 2.8 Tool-call cards reuse `render_tool_call`

The curator calls MCP tools to answer the user. These tool calls render as
**tool-call cards** — the same `render_tool_call` from `ThreadView`. The
card shows: tool name, status (pending/running/done/error), raw input
(collapsible), output (markdown or raw), copy button, expand/collapse.

Direct `/tool_name args` invocations also render as tool-call cards (with
the user as the "caller" instead of the curator). This unifies the two input
paths into a single visual language.

### 2.9 The status bar moves to the activity bar

The current `render_status_bar` (gas gauge + regulation health) is a good
feature. It moves to the **activity bar** at the bottom of each
`KaskConversationView` (the same place the agent panel puts cancel/retry/
scroll-to-bottom), not above the input. The gas gauge becomes a small
indicator in the activity bar, alongside the cancel button and the
scroll-to-bottom button. Each tab has its own activity bar (it's part of
the `KaskThreadView`).

---

## 3. Architecture

### 3.1 Crate structure

The `kask_panel` crate grows. New files:

```
crates/kask_panel/src/
├── kask_panel.rs              # The Item shell: tab strip + active KaskConversationView
├── conversation_view.rs       # Fork of agent_ui::ConversationView (simplified)
├── thread_view.rs             # Fork of agent_ui::ThreadView (simplified — the big one)
├── message_editor.rs          # Fork of agent_ui::MessageEditor (simplified)
├── curator_session.rs         # CuratorSession trait + ToolScope + CuratorEvent
├── system_prompt.rs           # Loads + renders the per-tab Jinja2 system prompt
├── tab_strip.rs               # The MCP-server tab strip (ui::Tab components)
├── panel_button.rs            # (unchanged) status bar toggle button
├── kanban_view.rs             # (unchanged) separate center-pane Item
├── portfolio_view.rs          # (unchanged) separate center-pane Item
└── scenarios_view.rs          # (unchanged) separate center-pane Item
```

New `Cargo.toml` dependencies: `markdown`, `theme_settings`, `component`,
`futures` (already), `smol` (if needed for streams).

### 3.2 The `KaskPanel` shell

```rust
pub struct KaskPanel {
    _workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    active_tab: usize,
    threads: HashMap<usize, Entity<KaskConversationView>>,
    tab_strip_state: TabStripState,
    regulation_snapshot: RegulationSnapshot,
    status_fetching: bool,
}
```

The `Item` impl is preserved (tab content text, serialization, focus). The
`Render` impl becomes:

```rust
v_flex()
    .size_full()
    .track_focus(&self.focus_handle)
    .child(self.render_tab_strip(cx))                    // ← new
    .child(self.active_conversation_view(cx).clone())    // ← replaces render_messages + render_input + render_status_bar
```

`active_conversation_view` lazily constructs the `KaskConversationView` for
`active_tab` on first access and caches it in `threads` — the same pattern
as the agent panel's `current_messages` lazy init.

### 3.3 The `KaskConversationView`

A simplified fork of `agent_ui::ConversationView`. Per-tab state:

- A single `KaskThreadView` (the message list + editor).
- A `CuratorSession` (one per tab, holding that tab's conversation history).
- The tab's `ToolScope` (the MCP server's tools).
- The tab's system prompt (rebuilt when the tab is first constructed).

It does **not** have: server state machine (always "connected" — the
curator is always available), auth, elicitation, terminal, subagents.

### 3.4 The `CuratorSession` and the bridge

The `CuratorSession` trait (§2.6) is implemented in `main.rs` by a
`PanelCuratorSession` adapter, **one instance per tab**. It wraps:

- The `InferencePort` (for curator LLM calls, via
  `generate_stream_with_messages`).
- The `ToolPort` (for the curator to call MCP tools — OCAP-gated, reusing
  `PanelToolInvoker`'s `DelegationToken` machinery).
- A `tokio::sync::Mutex<Vec<ChatMessage>>` for that tab's conversation
  history.

The `send` method:
1. Appends the user message to this tab's history.
2. Prepends the per-tab system prompt as the leading `system` message.
3. Calls `InferencePort::generate_stream_with_messages` with the history +
   the tab's tool definitions.
4. Streams `InferenceStreamChunk`s back as `CuratorEvent::TextDelta` /
   `ThinkingDelta` / `ToolCall` / `Done`.
5. When the curator emits a `ToolCall`, the session calls `ToolPort::invoke`
   (OCAP-gated), emits `CuratorEvent::ToolResult`, and appends the tool call
   + result to the history so the next turn sees it.
6. Appends the final assistant response to the history.

A `set_curator_session_factory` OnceLock replaces `set_scoped_inference`.
The factory is called per-tab to construct a fresh `PanelCuratorSession`
with its own history mutex. This is wired in the deferred task in `main.rs`
(per the `.rules` "Model-dependent kask wiring must run in the deferred
task" trap), with a `log::warn!` in the failure branch naming the hook
(per the "process-global hooks need a startup-failure signal" trap).

### 3.5 Persistence (v1: none; v2: ThreadStore-like)

v1 keeps conversations in-memory (lost on restart), matching the current
behavior. v2 adds a `KaskThreadStore` (fork of `ThreadStore`) with KVP
serialization, keyed by workspace + server ID. Deferred — the user's ask is
about interaction quality, not persistence.

### 3.6 System-prompt placement (per turn, v1)

Send `[system(tab_prompt), ...history, user]` each turn. Simple,
model-agnostic. The tool list is ~1-2k tokens; the cost is acceptable. v2
can optimize to a single system message at session start if the curator
model supports mid-conversation system updates.

---

## 4. Migration Plan (phased)

### Phase 1: Fork the conversation surface (no behavior change yet)

1. Copy `conversation_view.rs` → `kask_panel/src/conversation_view.rs`,
   `thread_view.rs` → `kask_panel/src/thread_view.rs`, `message_editor.rs`
   → `kask_panel/src/message_editor.rs`. Strip ACP/agent-server/terminal/
   elicitation/subagent/profile/model-selector code. Keep rendering.
2. Add `markdown`, `theme_settings`, `component` to `kask_panel/Cargo.toml`.
3. Define `CuratorSession`, `ToolScope`, `CuratorEvent` in
   `kask_panel/src/curator_session.rs`.
4. Define `set_curator_session_factory` OnceLock (replaces
   `set_scoped_inference`).
5. Wire `KaskPanel` to host one `KaskConversationView` per tab (still using
   the old `ScopedInference` under the hood, adapted to `CuratorSession`).
6. **No visual change yet** — the panel still looks the same, but the
   plumbing is the forked stack.

**Validation:** `cargo check -p kask_panel`, existing tests pass, panel opens
and behaves as before.

### Phase 2: Rich markdown rendering

1. Replace `render_messages`'s `Label::new` with `render_agent_markdown`
   (lifted from `agent_ui::conversation_view::render_agent_markdown`).
2. Create `Entity<Markdown>` per assistant message; update on streaming
   chunks.
3. Add `CopyButton` to each message.
4. Add `Callout` for system/error messages (replacing the `[system]` prefix
   label).
5. Add `WithRemSize` + `agent_ui_font_size` for font-size parity.

**Validation:** Visual parity with agent panel for markdown content. Manual
test: send a message that returns markdown (headers, code blocks, links,
lists) — renders identically to the agent panel.

### Phase 3: Tool-call cards

1. Port `render_tool_call` from `ThreadView` (expand/collapse, raw input,
   output markdown, error, copy).
2. Wire `CuratorSession::send` to emit `CuratorEvent::ToolCall` /
   `ToolResult`; render them as tool-call cards in the list.
3. Direct `/tool_name args` invocations render as tool-call cards too.

**Validation:** Curator calls a tool → card appears with input/output. User
runs `/tool_name args` → card appears. Expand/collapse works. Copy works.

### Phase 4: The tab strip + per-tab threads

1. Replace `render_server_selector` with `tab_strip.rs` (`ui::Tab`
   components, one per `BUILT_IN_MCP_SERVERS_IDS`).
2. `KaskPanel` holds `HashMap<usize, Entity<KaskConversationView>>`; tab
   switch swaps the active view.
3. Each tab's `KaskConversationView` is constructed with its own
   `CuratorSession` (own history) and `ToolScope` (the tab's server).

**Validation:** Clicking a tab switches the active thread. Each tab has its
own independent conversation. Direct `/tool` invocations hit the active
tab's server.

### Phase 5: Streaming + retry/cancel

1. Replace `ScopedInference::infer` (single `String`) with
   `CuratorSession::send` (stream of `CuratorEvent`).
2. Port `cancel_generation`, `retry`, `undo_last_reject` from `ThreadView`.
3. Port the activity bar (`render_activity_bar`) with cancel +
   scroll-to-bottom + gas gauge (from `render_status_bar`).

**Validation:** Streaming text appears incrementally. Cancel stops
generation. Retry re-sends the last message.

### Phase 6: System-prompt templates

1. Move `build_system_prompt` to `system_prompt.rs` with Jinja2 templates in
   `kask/registry/panel-prompts/`.
2. Templates receive `{{ server }}`, `{{ tools }}`, `{{ task }}`,
   `{{ curator_guidance }}`.
3. The curator guidance include (`panel-curator-guidance.j2`) is appended to
   every tab's system prompt.

**Validation:** Templates render correctly for all 10 servers. Curator
guidance is present in every tab.

### Phase 7: Message editor parity

1. Port `MessageEditor` → `KaskMessageEditor` with mentions (`@` for files/
   symbols), slash commands (`/tool_name` from the active tab's tools),
   context chips, expand-to-fullscreen, queue.
2. Replace `KaskToolCompletionProvider` with the forked completion path.

**Validation:** `@` mentions work. `/` slash-command menu shows the active
tab's tools. Expand works. Queue works.

### Phase 8: Polish + tests

1. Drag-and-drop files (`render_drag_target`).
2. Font-size actions (cmd-+/cmd-).
3. Scroll actions (page-up/down, to-top/bottom).
4. Tests pinning every deliberate deviation from `ThreadView` (per the
   `.rules` "tests must pin deliberate zed-kask deviations" trap).
5. Tests pinning the tab-strip behavior, per-tab thread independence, and
   that the curator's cross-tab recall flows through the curator server's
   memory (not through panel-side event forwarding).

---

## 5. Risks & Mitigations

| Risk | Mitigation |
|---|---|
| Forking 12k lines of `ThreadView` creates a maintenance burden (upstream changes don't flow). | The fork is a *simplifying* fork — we delete ~40% of the code. The remaining rendering code is stable (markdown, list, scroll, copy). Upstream changes to rendering are rare; when they happen, cherry-pick. |
| Per-tab `CuratorSession` instances each hold a conversation history — memory growth with 10 tabs. | Histories are bounded by the conversation length per tab; the curator server's episodic memory is the long-term store. v2 can add history truncation / condensation (the condenser MCP server exists for this). |
| The curator's cross-tab recall depends on the curator server's memory being populated. If the server is misconfigured (no `EpisodicMemory`), recall silently fails. | The curator server's `curator_ping` tool reports store availability (`episodic`, `semantic` flags). The panel can call it on first tab activation and warn if memory is unavailable (per the "process-global hooks need a startup-failure signal" trap). |
| Per-tab system prompts re-sent every turn is expensive. | Acceptable for v1 (§3.6). v2 optimizes. |
| The tab strip doesn't persist (conversations lost on restart). | v1 matches current behavior. v2 adds `KaskThreadStore`. |
| `KaskToolCompletionProvider` is replaced — existing tests break. | Port the tests to the new `KaskMessageEditor` completion path. |

---

## 6. What was wrong with v1 (recorded for the audit trail)

The v1 design doc proposed:
1. **A single shared curator conversation across all tabs** — wrong. Each
   tab is its own thread (`HashMap<ServerId, Entity<KaskConversationView>>`),
   mirroring the agent panel's `retained_threads`. Thread independence is
   the agent panel's core abstraction and the user's explicit directive
   ("each tab is a context focused mini-replica of the agent panel").
2. **A panel-side `observe_tool_use` forwarding layer** for cross-tab
   observation — wrong. The curator MCP server already owns `EpisodicMemory`
   and `SemanticMemory`, and `McpRuntime` already records every governed
   tool call's outcome in the `RegulationLedger`. Cross-tab observation is a
   property of the curator server's storage, not the panel's UI state. Adding
   a forwarding layer would duplicate the curator server's memory and violate
   thread independence.
3. **Open questions about curator session scope and mention scope** — these
   were not open questions; the user's directive ("each tab is a context
   focused mini-replica of the agent panel") answers them. The curator
   session is per-tab (one thread per tab); mentions are per-tab (scoped to
   the active tab's server domain).

The v1 design asked the user to resolve questions that the codebase and the
directive already answered. v2 corrects this by mapping the directive onto
the agent panel's existing thread abstraction and verifying the curator
server's cross-tab memory mechanism in the code.

---

## 7. Summary

The redesign is a **fork-and-simplify** of the agent panel's conversation
surface (`ConversationView` + `ThreadView` + `MessageEditor`) into the
`kask_panel` crate. The `KaskPanel` center-pane `Item` hosts a tab strip at
the top (one `ui::Tab` per built-in kask MCP server) and a stack of
per-tab `KaskConversationView`s below it — the structural mirror of the
agent panel's `retained_threads: HashMap<ThreadId, Entity<ConversationView>>`,
keyed by server ID. Each tab is an independent curator thread scoped to one
MCP server's tools and domain via a per-tab system-prompt template. The
curator observes across tabs because the curator MCP server owns
`EpisodicMemory` + `SemanticMemory` and `McpRuntime` records every tool
call's outcome in the `RegulationLedger` — no panel-side event forwarding.
The biggest new piece is the per-tab `CuratorSession` (stateful, streaming
via `InferencePort::generate_stream_with_messages`, tool-scoped). The fork
deletes ~40% of `ThreadView` (ACP, auth, elicitation, terminal, subagents,
profiles, model selector) and keeps the rendering (markdown, tool cards,
scroll, copy, retry). The result is pixel-parity with the agent panel's
conversation experience, plus multi-tab per-server context locking with the
curator as the single agent.
