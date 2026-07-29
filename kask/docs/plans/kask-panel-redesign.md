---
title: "Kask Panel Redesign — Agent-Panel-Parity Multi-Tab MCP Curator"
audience: [zed-kask integrators, hKask architects, GPUI engineers]
last_updated 2026-07-28
version: "0.1.0"
status: "Draft"
domain: "composition"
mds_categories: [composition, trust, lifecycle, curation]
---

# Kask Panel Redesign — Agent-Panel-Parity Multi-Tab MCP Curator

> **One-line frame:** Rebuild the kask panel as a multi-tabbed, context-locked
> agent panel where each tab is a single MCP server's domain, the only agent
> the user talks to is the **curator** (who observes MCP usage patterns and
> tool use across tabs), and the rest of the panel — rich markdown rendering,
> message editor with mentions/slash-commands, streaming, tool-call cards,
> activity bar, retry/cancel, copy, scroll behavior — is lifted from the
> existing `agent_ui::AgentPanel` / `ConversationView` / `ThreadView` stack
> rather than re-implemented.

## 0. Problem Statement

The current `kask_panel::KaskPanel` (`crates/kask_panel/src/kask_panel.rs`)
is a center-pane `Item` with an **impoverished** interaction model:

- Messages are rendered as `Label::new(format!("{prefix}{}", msg.content))`
  — plain text, no markdown, no code highlighting, no links, no copy buttons,
  no collapsible tool calls, no mermaid, no images. (See `render_messages` at
  L735–804.)
- The "input" is a bare `Editor` in `AutoHeight` mode with a hand-rolled
  `KaskToolCompletionProvider` that only completes `/tool_name` from a cache
  populated by `/tools`. No mentions, no context chips, no slash-command menu,
  no queue, no expand-to-fullscreen (`cmd-option-esc`), no follow-up mode.
- The conversation is a `Vec<KaskMessage>` of `(role, content: String)` — no
  streaming, no tool-call entries, no thinking blocks, no token usage, no
  retry, no edit-and-resend, no persistence.
- The "tabs" are a row of `Button`s in `render_server_selector` (L699–733)
  that swap `selected_server: usize` and switch `conversations: HashMap<usize,
  Vec<KaskMessage>>`. They are not real workspace tabs — they don't appear in
  the pane tab bar, can't be dragged, can't be closed individually, and don't
  carry per-tab state like a real `Item`.

The user's ask is to make the kask panel **behave like the agent panel**:
rich markdown, the same prompting/interaction patterns, the same response
presentation — but with two constraints:

1. **Multi-tabbed at the top** — one tab per MCP server (the 10 built-in
   `BUILT_IN_MCP_SERVERS_IDS`), plus the framing of each tab's context via a
   system-prompt template.
2. **Locked to the curator agent** — the user interacts with the curator, who
   can observe and learn from MCP usage patterns and tool use across the tabs.
   The curator is the only conversational agent; the MCP servers are tools the
   curator calls, not separate chat partners.

The user explicitly suggests: *"the easiest way may be to fork or clone the
agent panel and then make some tweaks or simplifications and then additions."*

This plan evaluates that suggestion against the actual code and concludes:
**fork-and-stretch is the right move**, but the fork is at the
`ConversationView` / `ThreadView` layer, not the `AgentPanel` dock layer —
because the kask panel is a center-pane `Item` (like `TerminalView`), not a
dock `Panel` (like `AgentPanel`). The dock shell is irrelevant; the
conversation surface is what matters.

---

## 1. Research Findings (GPUI, agent panel, visual language)

### 1.1 The agent panel's conversation stack

The rich interaction the user wants lives in **three crates** that the kask
panel currently does not touch:

| Layer | File | Role |
|---|---|---|
| `AgentPanel` (dock shell) | `crates/agent_ui/src/agent_panel.rs:1153` | Dock `Panel`, toolbar, thread list, terminal integration, onboarding, zoom. **Not relevant** — kask panel is a center-pane `Item`. |
| `ConversationView` | `crates/agent_ui/src/conversation_view.rs:591` | Per-thread view: server state machine (Loading/LoadError/Connected), auth, elicitation, focus, code-span resolver. Holds a `ThreadView`. |
| `ThreadView` | `crates/agent_ui/src/conversation_view/thread_view.rs:565` | The actual message list (`ListState`), message editor, tool-call cards, thinking blocks, markdown rendering, scroll, retry, cancel, copy, search. **This is the 12k-line file the user wants to mirror.** |
| `MessageEditor` | `crates/agent_ui/src/message_editor.rs:202` | Editor with mentions, slash commands, context chips, queue, expand, follow-up mode. |
| `render_agent_markdown` | `crates/agent_ui/src/conversation_view.rs:3386` | Shared helper: `MarkdownElement::new` + `CodeBlockRenderer` + image resolver + url-click + code-span-link. **Reusable as-is.** |
| `Markdown` / `MarkdownElement` / `MarkdownStyle` | `crates/markdown/src/markdown.rs` | The markdown crate. `MarkdownStyle::themed(MarkdownFont::Agent, window, cx)` is the agent-panel font/style. **Reusable as-is.** |

### 1.2 What the agent panel does that the kask panel doesn't

| Capability | Agent panel | Current kask panel |
|---|---|---|
| Message rendering | `MarkdownElement` with syntax highlighting, copy buttons, wrap buttons, mermaid, images, link-click, code-span file links | `Label::new(format!("{prefix}{}", msg.content))` |
| Message list | `ListState`-backed virtualized list with `cx.processor` per entry | `div().children(message_elements)` — flat, not virtualized |
| Streaming | `AssistantMessageChunk::Message { block, .. }` with live `Entity<Markdown>` updates | Single `String` returned by `ScopedInference::infer` — no streaming |
| Tool calls | `render_tool_call` cards with expand/collapse, permission prompts, raw input/output, error states | `format!("{tool}\n{formatted}")` in a `Label` |
| Thinking blocks | `AssistantMessageChunk::Thought` rendered collapsibly | None |
| Input editor | `MessageEditor` with mentions (`@`), slash commands (`/`), context chips, queue, expand, follow-up | Bare `Editor` + `KaskToolCompletionProvider` |
| Mentions | `MentionSet` (`crates/agent_ui/src/mention_set.rs`) resolving files/symbols/diagnostics/symbols | None |
| Slash commands | `SessionCapabilities` + `available_commands` from the agent server | Hard-coded `/help`, `/clear`, `/tools` |
| Retry / cancel | `cancel_generation`, `retry`, `undo_last_reject` | None (busy flag only) |
| Copy | `CopyButton` on every message + `CopyThreadToClipboard` | None |
| Scroll | `ListState::scroll_to_end`, page-up/down, scroll-to-message | `ScrollHandle::scroll_to_bottom` |
| Persistence | `ThreadStore` + KVP serialization | `HashMap<usize, Vec<KaskMessage>>` in memory, lost on restart |
| Token usage | `TokenUsageTooltip`, `last_token_limit_telemetry` | None |
| Profiles / modes | `ProfileSelector`, `ModeSelector`, `ConfigOptionsView` | None |
| Model selector | `ModelSelectorPopover` | None |
| Activity bar | `render_activity_bar` (cancel, retry, scroll-to-bottom) | None |
| Error rendering | `render_thread_error` with `Callout` + dismiss | `Label::new("Error: {error}")` |
| Font size | `WithRemSize` + `agent_ui_font_size` + cmd-+/cmd- | None |
| Drag-and-drop files | `render_drag_target` + `ExternalPaths` | None |

### 1.3 GPUI primitives the kask panel would need

- **`ListState`** (`crates/gpui/src/elements/list.rs:52`) — virtualized list
  with `set_scroll_handler`, `scroll_to_end`, `scroll_to`, `viewport_bounds`.
  Required for any non-trivial message count.
- **`Markdown` entity** — `cx.new(|cx| Markdown::new(source, language_registry,
  fallback_lang, cx))`. Holds the parsed source; re-parses on theme change.
- **`MarkdownElement`** — `MarkdownElement::new(md, style)` with
  `.code_block_renderer(...)`, `.image_resolver(...)`, `.on_url_click(...)`,
  `.on_code_span_link(...)`. This is what `render_agent_markdown` returns.
- **`MarkdownStyle::themed(MarkdownFont::Agent, window, cx)`** — the exact
  style the agent panel uses. `MarkdownFont::Agent` uses
  `agent_buffer_font_size` and `agent_ui_font_size` from `ThemeSettings`.
- **`Callout`** (`ui::Callout`) — for errors/warnings with severity, icon,
  title, description, dismiss action.
- **`CopyButton`** (`ui::CopyButton`) — copy-to-clipboard with tooltip.
- **`ContextMenu` / `PopoverMenu`** — for slash-command menus, model selectors.
- **`Editor` with `CompletionProvider`** — already used; the kask panel's
  `KaskToolCompletionProvider` is a minimal version of the agent panel's
  completion path.
- **`FocusHandle` + `track_focus`** — already used.
- **`cx.spawn` / `cx.background_spawn`** — already used for inference/tool
  tasks. The kask panel's `OnceLock`-based `ScopedInference` / `ToolInvoker`
  traits are the right abstraction; the bridge adapters in `main.rs`
  (`PanelScopedInference`, `PanelToolInvoker`) are reusable.

### 1.4 The center-pane `Item` vs dock `Panel` distinction

This is the single most important architectural fact for the redesign:

- `AgentPanel` implements `workspace::dock::Panel` — it lives in a dock
  (left/right/bottom), has a `toggle_action` (`ToggleFocus`), and is a
  singleton per workspace.
- `KaskPanel` implements `workspace::item::Item` — it lives in the center pane
  (like `TerminalView`, `Editor`), can have multiple instances, and is
  deployed via `workspace.add_item_to_active_pane`.

**The kask panel must remain a center-pane `Item`.** The user said "the rest
of the panel should behave the same way the user has learned to expect an
agentic AI panel to behave in zed" — but the *placement* (center pane, not
dock) is already correct and matches `TerminalView`. What needs to change is
the *content* of the `Item`, not the `Item` shell.

This means the fork target is **`ConversationView` + `ThreadView`**, not
`AgentPanel`. `AgentPanel`'s dock machinery (toolbar with agent selector,
terminal integration, onboarding upsell, zoom, flexible size, dock position
settings) is irrelevant noise. The kask panel's `Item` impl (tab content,
serialization, focus) is already fine and should be preserved.

### 1.5 The "tabs at the top" requirement

The user wants tabs at the top of the panel — one per MCP server. There are
two ways to interpret this:

**Option A: Workspace tabs (one `Item` per server).**
Open 10 `KaskPanel` items, one per server, each with its own tab in the
center-pane tab bar. The "tabs at the top" are the workspace's own tab bar.
- Pro: Reuses the workspace's tab management (drag, close, persist, restore).
- Pro: Each tab is a real `Item` with its own focus, serialization, history.
- Con: The user said "the rest of the panel should behave the same way" —
  implying a single panel with internal tabs, not 10 separate items.
- Con: The curator is supposed to observe across tabs; 10 separate items
  don't share state unless we add a shared curator session.
- Con: Workspace tabs are per-pane, not "at the top of the panel."

**Option B: In-panel tab strip (one `Item`, internal tab state).**
A single `KaskPanel` `Item` renders a tab strip at the top (like a browser),
switching the active server. Each tab carries its own conversation state.
- Pro: Matches the user's mental model ("tabs at the top of the panel").
- Pro: Single `Item`, single focus, single curator session observing all tabs.
- Pro: The current `render_server_selector` is already a (bad) version of
  this — it's a row of buttons. Upgrading it to a real tab strip is a
  focused change.
- Con: Doesn't reuse workspace tab persistence; we must persist per-tab
  conversation state ourselves (or accept that conversations are ephemeral,
  as they are today).

**Decision: Option B.** The user's framing ("beyond the tabs at the top and
the limitation to the curator agent, the rest of the panel should behave the
same way") makes this unambiguous — the tabs are *inside* the panel, and the
panel is a single `Item`. The curator observes across tabs because the
`KaskPanel` entity owns all tab state and forwards tool-use observations to
a single curator session.

### 1.6 The "locked to the curator" requirement

The user wants the only conversational agent to be the **curator**. The MCP
servers are tools the curator calls, not separate chat partners. This is a
change from the current design, where `ScopedInference::infer(server, prompt,
system_prompt)` makes the LLM "act as" the selected server.

The new model:

- There is **one** curator agent session per `KaskPanel` (or per workspace —
  see §3.3). The curator is the agent the user talks to in every tab.
- Each tab selects **which MCP server's tools are in scope** for that
  conversation. The tab's system prompt tells the curator: "You are operating
  in the {server} domain. You have these tools: [...]. The user is talking to
  you about {server-domain}. Use {server}'s tools to answer."
- The curator can observe tool-use patterns across tabs (e.g., "the user
  keeps asking the codegraph tab about call graphs — surface that as a
  learned pattern"). This is the "curator who can learn from and observe the
  mcp usage patterns and tool use" part.
- Direct `/tool_name args` invocation still bypasses the curator and calls
  the MCP tool directly — this is preserved as a power-user escape hatch.

This means the `ScopedInference` trait is **replaced** (or wrapped) by a
curator-session abstraction that:
1. Holds a persistent conversation with the curator (not stateless
   `infer(prompt, system_prompt)` calls).
2. Accepts a per-tab tool scope (which MCP server's tools are available).
3. Reports tool-use events back to the curator's observation memory.

The current `PanelScopedInference` in `main.rs` (L2221) builds a fresh
`[system, user]` message array per call — stateless. The redesign needs a
stateful curator session. This is the biggest new piece of infrastructure.

---

## 2. Design Decisions (pinned)

### 2.1 Fork `ConversationView` + `ThreadView`, not `AgentPanel`

The new kask panel will have a `KaskConversationView` (fork of
`ConversationView`) and a `KaskThreadView` (fork of `ThreadView`), living in
the `kask_panel` crate. The `KaskPanel` `Item` shell stays; it now hosts a
`KaskConversationView` instead of the hand-rolled `render_messages` /
`render_input` / `render_status_bar`.

**Why fork instead of reuse?**
- `ConversationView` is hard-wired to `agent_client_protocol` (ACP) — it
  speaks `acp::SessionId`, `acp::ToolCallId`, `AcpThread`, `AgentServer`.
  The kask panel does not use ACP; it uses `ScopedInference` + `ToolInvoker`
  traits. Adapting `ConversationView` to a non-ACP backend means either
  implementing an ACP shim (huge) or forking (manageable).
- `ThreadView` is 12k lines and tightly coupled to `AcpThread` events. A fork
  lets us delete the ACP event plumbing and replace it with the kask
  `ScopedInference` / `ToolInvoker` traits, while keeping the rendering
  (markdown, tool cards, scroll, copy) intact.
- The fork is a **simplification**: we delete ACP, agent-server auth,
  elicitation, terminal integration, subagents, profiles, model selector,
  thinking-effort menus, fast mode, trial upsell, onboarding, and the thread
  store. What remains is the conversation surface + markdown + tool cards.

**Why not build from scratch?**
- The user explicitly asked to avoid that ("the easiest way may be to fork or
  clone the agent panel"). Re-implementing `render_markdown`, `ListState`
  wiring, tool-call cards, scroll, copy, retry from scratch would take weeks
  and diverge from the agent panel's visual language.
- The agent panel's visual language (markdown style, callout severity, copy
  button placement, scroll behavior, message editor) is what the user wants
  the kask panel to match. Forking is the only way to guarantee pixel-parity.

### 2.2 The fork is a *simplifying* fork

The forked `KaskThreadView` will be **smaller** than `ThreadView`, not larger.
We delete:

- All ACP event handling (`AcpThreadEvent`, `ThreadStatus`, `SessionId`).
- Agent-server auth (`AuthState`, `auth_methods`, `reauthenticate`, `logout`).
- Elicitation (`render_request_elicitations`, `ElicitationFormState`).
- Terminal integration (`render_terminal`, `TerminalThreadMetadata`).
- Subagents (`render_subagent_titlebar`, `parent_session_id` navigation).
- Profiles / modes / thinking-effort / fast-mode menus.
- Model selector (`ModelSelectorPopover`).
- Trial upsell / onboarding / codex-windows warning.
- Thread store / KVP persistence (v1 — see §3.5).
- Buffer search bar (`ThreadSearchBar`) — v1 skips in-thread search.
- Multi-root callout, external-source-prompt warning.

What we **keep** from `ThreadView`:

- `ListState`-backed message list with `render_entries`.
- `render_entry` → `render_markdown` (via `render_agent_markdown`).
- `render_tool_call` cards (expand/collapse, raw input, output markdown,
  error states, copy buttons).
- `render_thinking_block` (if the curator model supports thinking).
- `render_generating` spinner.
- `render_message_editor` (forked to `KaskMessageEditor`).
- Scroll actions (page-up/down, to-top/bottom, to-message).
- Copy / retry / cancel actions.
- `render_thread_error` with `Callout`.
- `WithRemSize` font-size handling.
- Drag-and-drop file target (`render_drag_target`).

### 2.3 The tab strip replaces `render_server_selector`

The current `render_server_selector` (L699–733) is a `v_flex` with a "MCP
Server" label, a wrapping row of `Button`s, and a "Selected: {current}"
label. This becomes a proper tab strip at the top of the panel:

- One tab per `BUILT_IN_MCP_SERVERS_IDS` entry (10 tabs).
- The active tab is highlighted; clicking switches the active conversation.
- Each tab shows the server icon + name.
- The tab strip is part of the `KaskPanel` render (above the
  `KaskConversationView`), not the workspace tab bar.
- The tab strip is a `h_flex` of `Tab` components (`ui::Tab`) — the same
  component used in the agent panel's toolbar and settings pages. This
  matches the visual language.

### 2.4 The curator is the only agent; tabs are tool scopes

- The `KaskPanel` holds **one** curator conversation session
  (`KaskConversationView`), not 10. The conversation is continuous across
  tab switches — the user is talking to the curator the whole time.
- Each tab switch changes the **tool scope** passed to the curator: "You are
  now in the {server} domain. Your available tools are: [...]."
- The conversation history is **shared** across tabs (the curator remembers
  what was said in the codegraph tab when the user switches to the research
  tab). This is the "curator observes across tabs" behavior.
- Direct `/tool_name args` invocation still works per-tab — it calls the
  active tab's MCP server directly, bypassing the curator, and the result is
  inserted into the shared conversation as a tool-call entry.
- The tab's system prompt is built by a **system-prompt template** (see §2.5)
  that frames the server's domain and tool list.

**Alternative considered:** per-tab separate conversations (the current
`HashMap<usize, Vec<KaskMessage>>` model). Rejected because the user wants
the curator to "learn from and observe the mcp usage patterns and tool use"
across tabs — separate conversations can't do that.

### 2.5 System-prompt template per tab

The current `build_system_prompt` (L224–272) is a Rust `format!` string. The
redesign replaces it with a **Jinja2 template** (`.j2`) loaded by the
template root, matching the kask skill-cascade pattern. The template:

- Receives `{{ server }}`, `{{ server_description }}`, `{{ tools }}` (list
  of `{name, description}`), and `{{ curator_guidance }}`.
- Emits the system prompt that frames the curator's role in this tab's
  domain.
- Lives in `kask/registry/panel-prompts/{server}.j2` or a single
  `panel-tab-system.j2` parameterized by server.
- The curator-specific guidance (remembering issues, observing tool-use
  patterns) is a shared include (`panel-curator-guidance.j2`) appended to
  every tab's system prompt.

This aligns with the `.rules` trap "Skill cascade context must carry the
user's task" — the template receives the user's task as `{{ task }}` so the
curator's framing includes what the user is doing right now.

### 2.6 The `ScopedInference` trait evolves to a curator session

The current `ScopedInference::infer(server, prompt, system_prompt)` is
stateless. The redesign introduces a `CuratorSession` trait:

```rust
pub trait CuratorSession: Send + Sync {
    /// Send a user message to the curator with a tool scope.
    /// Returns a stream of curator events (text chunks, tool calls,
    /// tool results, thinking, done).
    fn send(
        &self,
        message: &str,
        tool_scope: &ToolScope,
        system_prompt: &str,
    ) -> Task<Result<CuratorEventStream, String>>;

    /// Cancel the in-flight curator turn.
    fn cancel(&self) -> Task<Result<()>>;

    /// Observe a direct tool invocation (from `/tool_name args`) so the
    /// curator can learn from it even when bypassed.
    fn observe_tool_use(&self, event: ToolUseEvent) -> Task<()>;
}

pub enum ToolScope {
    /// Only the named MCP server's tools are available.
    Server(String),
    /// All MCP servers' tools are available (cross-tab).
    All,
}
```

The bridge (`main.rs`) provides the implementation, wrapping the existing
`InferencePort` with a stateful message history and the `ToolPort` for tool
calls. This is the biggest new piece — see §3.4.

### 2.7 Streaming is required for parity — and the port already supports it

The agent panel streams (`AssistantMessageChunk::Message { block, .. }` with
live `Entity<Markdown>` updates). The current kask panel's
`ScopedInference::infer` returns a single `String` — no streaming. The
redesign requires streaming for parity. The `CuratorSession::send` returns a
`CuratorEventStream` (a `BoxStream<CuratorEvent>`) that the
`KaskThreadView` consumes incrementally, updating the active
`Entity<Markdown>` as chunks arrive. This is the same pattern as
`AcpThread` events → `ThreadView` re-render.

**Resolved:** `hkask_types::InferencePort::generate_stream_with_messages`
(`kask/crates/hkask-types/src/ports/inference_port.rs:78-89`) returns
`Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>>>>`.
`InferenceStreamChunk` (L186–195) carries `text_delta`, `reasoning_delta`
(thinking), `tool_calls: Vec<StructuredToolCall>`, `finish_reason`, and
`usage: Option<InferenceUsage>`. Streaming is fully supported — including
structured tool calls and thinking deltas — so the `CuratorEvent` enum can
mirror `InferenceStreamChunk`'s fields directly. No v1 fallback needed.

### 2.8 Tool-call cards reuse `render_tool_call`

The curator calls MCP tools to answer the user. These tool calls render as
**tool-call cards** — the same `render_tool_call` from `ThreadView`. The
card shows: tool name, status (pending/running/done/error), raw input
(collapsible), output (markdown or raw), copy button, expand/collapse.

Direct `/tool_name args` invocations also render as tool-call cards (with
the user as the "caller" instead of the curator). This unifies the two input
paths into a single visual language.

### 2.9 The status bar is preserved but repositioned

The current `render_status_bar` (gas gauge + regulation health) is a good
feature. It moves to the **activity bar** at the bottom of the
`KaskConversationView` (the same place the agent panel puts cancel/retry/
scroll-to-bottom), not above the input. The gas gauge becomes a small
indicator in the activity bar, alongside the cancel button and the
scroll-to-bottom button.

---

## 3. Architecture

### 3.1 Crate structure

The `kask_panel` crate grows. New files:

```
crates/kask_panel/src/
├── kask_panel.rs              # The Item shell (tab strip + KaskConversationView host)
├── conversation_view.rs       # Fork of agent_ui::ConversationView (simplified)
├── thread_view.rs             # Fork of agent_ui::ThreadView (simplified, the big one)
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
`smol` (for streams, if not already), `futures` (already), `agent_skills`
(for the curator to observe skill invocations — optional v1).

### 3.2 The `KaskPanel` shell

The `KaskPanel` struct becomes:

```rust
pub struct KaskPanel {
    _workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    /// The active tab (index into BUILT_IN_MCP_SERVERS_IDS).
    active_tab: usize,
    /// The single curator conversation view (shared across tabs).
    conversation: Entity<KaskConversationView>,
    /// Per-tab tool scope cache (which tools are available per server).
    /// Populated lazily by list_tools on first tab activation.
    tab_tool_scopes: HashMap<usize, Vec<ToolDescriptor>>,
    /// The tab strip element state.
    tab_strip_state: TabStripState,
    /// Latest regulation snapshot (for the activity bar gas gauge).
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
    .child(self.render_tab_strip(cx))           // ← new
    .child(self.conversation.clone())           // ← replaces render_messages + render_input + render_status_bar
```

### 3.3 The `KaskConversationView`

A simplified fork of `agent_ui::ConversationView`. It holds:

- A single `KaskThreadView` (the message list + editor).
- The `CuratorSession` (injected via a new `set_curator_session` OnceLock).
- The active `ToolScope` (switched by the tab strip).
- The active system prompt (rebuilt when the tab changes).

It does **not** have: server state machine (always "connected" — the
curator is always available), auth, elicitation, terminal, subagents.

### 3.4 The `CuratorSession` and the bridge

The `CuratorSession` trait (§2.6) is implemented in `main.rs` by a
`PanelCuratorSession` adapter, replacing `PanelScopedInference`. It wraps:

- The `InferencePort` (for curator LLM calls).
- The `ToolPort` (for the curator to call MCP tools).
- A `tokio::sync::Mutex<Vec<ChatMessage>>` (the curator's conversation
  history, persisted across `send` calls).
- The OCAP `DelegationToken` machinery (reused from `PanelToolInvoker`).

The `send` method:
1. Appends the user message to the history.
2. Appends the system prompt (per-tab) to the *front* of the history (or
   sends it as a system message each turn — TBD, see §3.6).
3. Calls `InferencePort::generate_with_messages` with the history + tool
   definitions (the active tab's tools).
4. Streams the response back as `CuratorEvent::TextChunk`, `CuratorEvent::ToolCall`,
   `CuratorEvent::ToolResult`, `CuratorEvent::Done`.
5. Appends the assistant response to the history.

The `observe_tool_use` method: when the user runs `/tool_name args`
directly, the panel calls this to record the tool use in the curator's
memory (so the curator can mention it in future turns).

### 3.5 Persistence (v1: none; v2: ThreadStore-like)

v1 keeps conversations in-memory (lost on restart), matching the current
behavior. v2 adds a `KaskThreadStore` (fork of `ThreadStore`) with KVP
serialization, keyed by workspace + panel instance. This is deferred because
the user's ask is about interaction quality, not persistence.

### 3.6 System-prompt placement (open question)

Two options for where the per-tab system prompt goes:

**A. Leading system message per turn.** Send `[system(tab_prompt), user,
...history]` each turn. Simple, stateless on the curator side. Cost: every
turn re-sends the (potentially long) tool list.

**B. System message once, then update.** Send the system prompt once at
session start, then send a "tool scope changed" system message when the tab
switches. Cheaper but requires the model to support mid-conversation system
updates (not all do).

**Decision: A for v1.** It's simpler and model-agnostic. The tool list is
~1-2k tokens; the cost is acceptable. v2 can optimize to B if the curator
model supports it.

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
4. Define `set_curator_session` OnceLock (replaces `set_scoped_inference`).
5. Wire `KaskPanel` to host a `KaskConversationView` (still using the old
   `ScopedInference` under the hood, adapted to `CuratorSession`).
6. **No visual change yet** — the panel still looks the same, but the
   plumbing is now the forked stack.

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
   `ToolResult` events; render them as tool-call cards in the list.
3. Direct `/tool_name args` invocations render as tool-call cards too.

**Validation:** Curator calls a tool → card appears with input/output. User
runs `/tool_name args` → card appears. Expand/collapse works. Copy works.

### Phase 4: The tab strip + tool scopes

1. Replace `render_server_selector` with `tab_strip.rs` (`ui::Tab`
   components, one per `BUILT_IN_MCP_SERVERS_IDS`).
2. Tab switch changes `ToolScope` passed to `CuratorSession::send` and
   rebuilds the system prompt.
3. Per-tab tool-scope cache (`tab_tool_scopes`) populated lazily by
   `list_tools` on first tab activation.

**Validation:** Clicking a tab switches the active tool scope. The curator
knows which tab the user is in. Direct `/tool` invocations hit the active
tab's server.

### Phase 5: Streaming + retry/cancel

1. Replace `ScopedInference::infer` (single `String`) with
   `CuratorSession::send` (stream of `CuratorEvent`).
2. Port `cancel_generation`, `retry`, `undo_last_reject` from `ThreadView`.
3. Port the activity bar (`render_activity_bar`) with cancel + scroll-to-bottom
   + gas gauge (from `render_status_bar`).

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
   symbols — scoped to the active tab's server where applicable), slash
   commands (`/tool_name` from the active tab's tools), context chips,
   expand-to-fullscreen, queue.
2. Replace `KaskToolCompletionProvider` with the forked completion path.

**Validation:** `@` mentions work. `/` slash-command menu shows the active
tab's tools. Expand works. Queue works.

### Phase 8: Polish + tests

1. Drag-and-drop files (`render_drag_target`).
2. Font-size actions (cmd-+/cmd-).
3. Scroll actions (page-up/down, to-top/bottom).
4. Tests pinning every deliberate deviation from `ThreadView` (per the
   `.rules` "tests must pin deliberate zed-kask deviations" trap).
5. Tests pinning the tab-strip behavior, tool-scope switching, curator
   observation memory.

---

## 5. Risks & Mitigations

| Risk | Mitigation |
|---|---|
| Forking 12k lines of `ThreadView` creates a maintenance burden (upstream changes don't flow). | The fork is a *simplifying* fork — we delete ~40% of the code. The remaining rendering code is stable (markdown, list, scroll, copy). Upstream changes to rendering are rare; when they happen, cherry-pick. |
| `CuratorSession` streaming requires `InferencePort` to support streaming. | **Resolved:** `generate_stream_with_messages` exists and returns `InferenceStreamChunk` with `text_delta`, `reasoning_delta`, `tool_calls`, `finish_reason`, `usage`. No blocker. |
| The curator's "observation memory" (learning from tool use across tabs) is underspecified. | v1: the curator's conversation history is the memory — it sees all tool calls in the shared history. v2: add a structured `observe_tool_use` event store. |
| Per-tab system prompts re-sent every turn is expensive. | Acceptable for v1 (§3.6). v2 optimizes. |
| The tab strip doesn't persist (conversations lost on restart). | v1 matches current behavior. v2 adds `KaskThreadStore`. |
| `KaskToolCompletionProvider` is replaced — existing tests break. | Port the tests to the new `KaskMessageEditor` completion path. |

---

## 6. Open Questions

1. **Curator session scope:** one `CuratorSession` per `KaskPanel` instance,
   or one per workspace (shared across multiple `KaskPanel` instances)?
   The user said "the context in the panel should be locked to the context of
   the tab" — suggesting per-panel. But "the curator observes across tabs"
   suggests a single curator. **Proposed: per-panel** (one curator per panel
   instance); if the user opens two panels, they get two curators. v2 can
   share.

2. **Streaming support (resolved).** `InferencePort::generate_stream_with_messages` returns a `Stream<Item = Result<InferenceStreamChunk, _>>` with `text_delta`, `reasoning_delta`, `tool_calls`, `finish_reason`, `usage`. The `CuratorSession::send` impl wraps this directly. `CuratorEvent` mirrors `InferenceStreamChunk`.

3. **Should direct `/tool_name args` invocations be per-tab or global?**
   Currently they're per-tab (the active server). The redesign preserves
   this. But should the user be able to call a tool from a *different* tab's
   server without switching tabs? v1: no — switch tabs first.

4. **Mention scope:** when the user types `@` in the codegraph tab, should
   mentions include codegraph-specific entities (functions, classes) or
   just files/symbols? v1: files/symbols only (same as agent panel).
   v2: per-server mention providers.

---

## 7. Summary

The redesign is a **fork-and-simplify** of the agent panel's conversation
surface (`ConversationView` + `ThreadView` + `MessageEditor`) into the
`kask_panel` crate, wrapped by the existing `KaskPanel` center-pane `Item`
shell with a new tab strip. The curator becomes the only conversational
agent; tabs switch the tool scope passed to the curator. The biggest new
piece is the `CuratorSession` trait (stateful, streaming, tool-scoped),
replacing the stateless `ScopedInference`. The fork deletes ~40% of
`ThreadView` (ACP, auth, elicitation, terminal, subagents, profiles, model
selector) and keeps the rendering (markdown, tool cards, scroll, copy,
retry). The result is pixel-parity with the agent panel's conversation
experience, plus multi-tab MCP-server context locking.
