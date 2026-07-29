---
title: "Kask Panel Architecture — Reuse Agent Panel, Don't Reinvent"
audience: [zed-kask integrators, hKask architects, GPUI engineers]
last_updated 2026-07-29
version: "0.3.0"
status: "Draft"
domain: "composition"
mds_categories: [composition, trust, lifecycle, curation]
---

# Kask Panel Architecture — Reuse Agent Panel, Don't Reinvent

> **One-line frame:** The kask panel should not be a separate panel with its
> own rendering code. It should be the **agent panel** with the curator agent
> pre-selected and a tab strip for MCP server context switching. The agent
> panel already has `Agent::Curator`, `CuratorAgentServer`,
> `ConversationView`, `ThreadView`, `MessageEditor`, `MarkdownElement`,
> `ListState`, `CopyButton`, retry/cancel, scroll, drag-and-drop, font-size
> actions — all the interaction patterns the kask panel needs. The kask panel
> should reuse these, not re-implement them.

## 0. The Problem

The current kask panel (`crates/kask_panel/src/kask_panel.rs`) is a
center-pane `Item` with its own:

- `Vec<KaskMessage>` conversation model (plain strings, not ACP threads)
- `Label::new` rendering for non-assistant messages (not selectable, not
  copyable — the immediate symptom the user reported)
- Hand-rolled `render_messages`, `render_input`, `render_status_bar`
- `KaskToolCompletionProvider` (minimal, no mentions, no slash-command menu)
- No streaming (the `CuratorSession` trait was added but the panel's
  rendering doesn't fully support it)
- No retry/cancel/copy/scroll-to-message/drag-and-drop/font-size

The agent panel (`crates/agent_ui/src/agent_panel.rs`) already has all of
this. It already has `Agent::Curator` as a selectable agent variant. It
already has `CuratorAgentServer` wired. The user can already select
"Curator" in the agent panel's agent selector and get a curator thread.

The kask panel's value-add over the agent panel is **the tab strip** —
one tab per MCP server, switching the tool scope and system prompt. That's
~50 lines of UI code on top of the agent panel, not a 1400-line
re-implementation.

## 1. What the Agent Panel Already Has

| Capability | Agent panel | Kask panel (current) |
|---|---|---|
| Message rendering | `MarkdownElement` (selectable, copyable, mermaid, code highlighting) | `Label::new` (not selectable) for non-assistant; `MarkdownElement` for assistant |
| Message list | `ListState`-backed virtualized list | `div().children()` — flat, not virtualized |
| Streaming | `AssistantMessageChunk` with live `Entity<Markdown>` | `CuratorSession` trait (wired but rendering incomplete) |
| Tool-call cards | `render_tool_call` (expand/collapse, raw input, output, copy) | `ToolCallCard` entity (partially implemented) |
| Input editor | `MessageEditor` (mentions, slash commands, context chips, queue, expand) | Bare `Editor` + `KaskToolCompletionProvider` |
| Retry / cancel | `cancel_generation`, `retry`, `undo_last_reject` | `CuratorSession::cancel` + `retry` (wired but no UI) |
| Copy | `CopyButton` on every message + `CopyThreadToClipboard` | `CopyButton` on assistant messages only |
| Scroll | `ListState::scroll_to_end`, page-up/down, scroll-to-message | `ScrollHandle::scroll_to_bottom` |
| Font size | `WithRemSize` + `agent_ui_font_size` + cmd-+/cmd- | None |
| Drag-and-drop | `render_drag_target` + `ExternalPaths` | None |
| Agent selection | `Agent::Curator` in the agent selector | Hardcoded curator |
| Thread persistence | `ThreadStore` + KVP | In-memory `HashMap` |

## 2. The Right Architecture

### Option A: KaskPanel hosts ConversationView (recommended)

The `KaskPanel` `Item` becomes a thin wrapper:

```rust
pub struct KaskPanel {
    workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    active_tab: usize,
    // One ConversationView per MCP server tab — mirrors the agent panel's
    // retained_threads: HashMap<ThreadId, Entity<ConversationView>>
    threads: HashMap<usize, Entity<ConversationView>>,
    tab_strip_state: TabStripState,
}
```

Each `ConversationView` is constructed with `Agent::Curator` and the
tab's MCP server as the tool scope. The `ConversationView` handles all
rendering, streaming, tool calls, retry, cancel, copy, scroll — the
kask panel just hosts it and provides the tab strip.

**What the kask panel adds:**
1. A tab strip at the top (one `ui::Tab` per `BUILT_IN_MCP_SERVERS_IDS`)
2. Tab switch logic (swap which `ConversationView` is rendered)
3. Per-tab system prompt (built from the server's tool list)

**What the kask panel does NOT add:**
- No custom message rendering
- No custom input editor
- No custom tool-call cards
- No custom scroll/status bar
- No custom conversation model

### Option B: KaskPanel IS an AgentPanel variant (more invasive)

Make `KaskPanel` a thin struct that delegates to `AgentPanel` methods,
pre-selects `Agent::Curator`, and adds the tab strip. This is more
invasive (requires `AgentPanel` to be usable outside a dock) but gives
maximum reuse.

**Not recommended** — `AgentPanel` is a dock `Panel`, and the kask panel
is a center-pane `Item`. The dock/Item distinction is correct and should
be preserved.

### Option C: Fork ConversationView + ThreadView (the original plan)

Fork `ConversationView` and `ThreadView` into `KaskConversationView`
and `KaskThreadView`, stripping ACP/agent-server/terminal/elicitiation.

**Not recommended** — this is what the current kask panel is trying to
do, and it's the source of the problem. Forking 12k lines of `ThreadView`
creates a maintenance burden and diverges from the agent panel's visual
language. The whole point is to NOT fork.

## 3. How to Implement Option A

### Step 1: Construct ConversationView with Agent::Curator

The agent panel's `create_agent_thread_inner` already constructs a
`ConversationView` with `Agent::Curator`. The kask panel can call the
same path:

```rust
fn ensure_thread_for_tab(&mut self, tab: usize, window: &mut Window, cx: &mut Context<Self>) {
    if self.threads.contains_key(&tab) {
        return;
    }
    let server = BUILT_IN_MCP_SERVERS[tab].id;
    // Construct a ConversationView with Agent::Curator
    // The ConversationView handles all rendering, streaming, tool calls
    let cv = cx.new(|cx| {
        ConversationView::new(
            Agent::Curator.server(self.fs.clone(), None),
            self.connection_store.clone(),
            Agent::Curator,
            None, // no resume
            None, // no thread_id
            None, // no work_dirs
            None, // no title
            None, // no initial_content
            self.workspace.clone(),
            self.project.clone(),
            None, // no thread_store
            AgentThreadSource::AgentPanel,
            window,
            cx,
        )
    });
    self.threads.insert(tab, cv);
}
```

### Step 2: The tab strip switches the active ConversationView

```rust
fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    self.ensure_thread_for_tab(self.active_tab, window, cx);
    v_flex()
        .size_full()
        .track_focus(&self.focus_handle)
        .child(self.render_tab_strip(cx))
        .child(self.threads[&self.active_tab].clone())
}
```

### Step 3: Per-tab system prompt

The `ConversationView` → `ThreadView` → `NativeAgent` path already
supports `static_context` (used by `CuratorAgentServer`). The per-tab
system prompt can be injected via the same mechanism — the kask panel
sets the `static_context` on the `NativeAgent` before the thread starts,
or the `CuratorAgentServer` reads the active tab's server from a
context variable.

### Step 4: Tool scope

The `NativeAgent` already has a `ToolRouter` that filters MCP tools.
The kask panel's tab strip sets the active server, and the
`ToolRouter` is configured to only expose that server's tools for
that tab's thread.

## 4. What to Delete

The current kask panel's custom rendering code:

- `render_messages` — replaced by `ConversationView`'s `ThreadView`
- `render_input` — replaced by `MessageEditor`
- `render_status_bar` — replaced by `ThreadView`'s activity bar
- `KaskMessage` struct — replaced by `AcpThread` entries
- `KaskMessageRole` enum — replaced by ACP entry types
- `KaskToolCompletionProvider` — replaced by `MessageEditor`'s completion
- `markdown_render.rs` — replaced by `render_agent_markdown` (already exists)
- `tool_call_card.rs` — replaced by `ThreadView`'s `render_tool_call`
- `CuratorSession` trait + `PanelCuratorSession` — replaced by
  `NativeAgent`'s streaming (already supports `generate_stream_with_messages`)
- `ToolInvoker` trait + `PanelToolInvoker` — replaced by `NativeAgent`'s
  `ToolRouter` (already OCAP-gated)

## 5. What to Keep

- `KaskPanel` struct (the `Item` shell + tab strip)
- `TabStripState` (the tab strip UI)
- `panel_button.rs` (the status bar toggle button)
- `KanbanBoardView`, `PortfolioDashboardView`, `ScenariosView` (separate
  center-pane items, not affected)
- The `Toggle`/`ToggleFocus` actions (already correct)

## 6. Implementation

No backward compatibility or migration is needed — the product is
pre-release. The existing kask panel code is replaced directly.

1. **Replace `KaskPanel` with a `ConversationView` host + tab strip.**
   Delete the custom rendering code (`render_messages`, `render_input`,
   `render_status_bar`, `KaskMessage`, `KaskMessageRole`,
   `KaskToolCompletionProvider`, `markdown_render.rs`,
   `tool_call_card.rs`, `CuratorSession` trait, `ToolInvoker` trait,
   `PanelCuratorSession`, `PanelToolInvoker`). The panel becomes a thin
   `Item` that hosts one `ConversationView` per tab.

2. **Add the tab strip.** Each tab creates a `ConversationView` with
   `Agent::Curator` and a different tool scope. Tab switch swaps the
   active view.

3. **Wire the per-tab system prompt** via `static_context`. The
   `CuratorAgentServer` reads the active tab's server and builds the
   system prompt.

4. **Wire the tool scope per tab.** The `ToolRouter` filters to only
   the active tab's server's tools.

## 7. Why This Is Correct

1. **Zero visual divergence.** The kask panel uses the exact same
   rendering code as the agent panel. Text selection, copy, scroll,
   retry, cancel, tool-call cards, markdown, mermaid — all inherited
   for free.

2. **Minimal maintenance burden.** The kask panel is ~100 lines (tab
   strip + ConversationView host), not ~1400 lines of custom rendering.
   When the agent panel's rendering improves, the kask panel inherits
   the improvement automatically.

3. **The curator is already an agent.** `Agent::Curator` +
   `CuratorAgentServer` + `NativeAgent` already exist and are wired.
   The kask panel just needs to use them, not reinvent them.

4. **The tab strip is the only value-add.** Everything else the kask
   panel does is a subset of what the agent panel already does. The
   tab strip (MCP server context switching) is the one thing the agent
   panel doesn't have, and it's ~50 lines of UI code.
