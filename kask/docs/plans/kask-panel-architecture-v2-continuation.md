# Continuation Prompt: Kask Panel Architecture v2 Implementation

You are implementing the kask panel architecture v2 plan documented at
`kask/docs/plans/kask-panel-architecture-v2.md`. Read that document first —
it explains why the current kask panel is wrong and what the replacement
architecture is.

## Context

The current kask panel (`crates/kask_panel/src/kask_panel.rs`) is a
center-pane `Item` with ~1400 lines of custom rendering code that
re-invents what the agent panel already does — badly. The user cannot
select or copy text, there's no streaming, no retry/cancel, no mentions,
no drag-and-drop. The agent panel (`crates/agent_ui/`) already has all of
this, and already has `Agent::Curator` as a selectable agent variant with
`CuratorAgentServer` wired.

The plan is to replace the kask panel's custom rendering with the agent
panel's `ConversationView` — making the kask panel a thin `Item` that hosts
one `ConversationView` per MCP server tab, with a tab strip for context
switching. No backward compatibility is needed — the product is
pre-release. Delete the old code and replace it directly.

## What to read first

1. `kask/docs/plans/kask-panel-architecture-v2.md` — the architecture plan
2. `crates/agent_ui/src/agent_panel.rs` lines 1153-1200 — the `AgentPanel`
   struct and its `retained_threads: HashMap<ThreadId, Entity<ConversationView>>`
   field (the pattern the kask panel should mirror)
3. `crates/agent_ui/src/agent_panel.rs` lines 4573-4650 —
   `create_agent_thread_inner` — how the agent panel constructs a
   `ConversationView` with `Agent::Curator`
4. `crates/agent_ui/src/conversation_view.rs` lines 591-616 — the
   `ConversationView` struct
5. `crates/agent_ui/src/conversation_view.rs` lines 799-830 —
   `ConversationView::new` constructor signature
6. `crates/agent/src/curator_agent_server.rs` — the `CuratorAgentServer`
   that wraps `NativeAgent` with curator context
7. `crates/agent_ui/src/agent_ui.rs` lines 426-460 — the `Agent` enum with
   `Agent::Curator` variant
8. `crates/kask_panel/src/kask_panel.rs` — the current code to be replaced
9. `crates/kask_panel/Cargo.toml` — current dependencies

## What to implement

### 1. Replace KaskPanel with a ConversationView host + tab strip

The new `KaskPanel` struct:

```rust
pub struct KaskPanel {
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    focus_handle: FocusHandle,
    active_tab: usize,
    threads: HashMap<usize, Entity<ConversationView>>,
    // ... minimal state for the tab strip
}
```

Each tab lazily constructs a `ConversationView` with `Agent::Curator`:
- The `ConversationView` is created via the same path the agent panel uses
  (`Agent::Curator.server(...)` → `ConversationView::new(...)`)
- The `ConversationView` handles ALL rendering: messages, input editor,
  tool-call cards, scroll, retry, cancel, copy, markdown, streaming
- The kask panel only adds the tab strip and tab-switch logic

### 2. Delete the old custom rendering code

Delete these from `kask_panel.rs` and `kask_panel/Cargo.toml`:
- `KaskMessage`, `KaskMessageRole` — replaced by ACP thread entries
- `render_messages`, `render_input`, `render_status_bar` — replaced by
  `ConversationView`'s rendering
- `KaskToolCompletionProvider` — replaced by `MessageEditor`'s completion
- `ToolDescriptor`, `ToolInvoker` trait, `ScopedInference` trait,
  `CuratorSession` trait, `CuratorEvent`, `ToolScope` — replaced by
  `NativeAgent`'s streaming + `ToolRouter`
- `RegulationSnapshot`, `RegulationStatus` trait — the status bar is
  part of `ThreadView`'s activity bar
- `build_system_prompt`, `server_welcome`, `server_description` — the
  system prompt is injected via `CuratorAgentServer`'s `static_context`
- `parse_tool_invocation`, `parse_args`, `format_json_result` — no longer
  needed (the `MessageEditor` handles slash commands, the `NativeAgent`
  handles tool dispatch)
- `markdown_render.rs` — replaced by `render_agent_markdown` from
  `agent_ui::conversation_view`
- `tool_call_card.rs` — replaced by `ThreadView`'s `render_tool_call`
- `curator_session.rs` — replaced by `NativeAgent`'s streaming

### 3. Add the tab strip

One `ui::Tab` per `BUILT_IN_MCP_SERVERS_IDS` entry (10 tabs). Clicking a
tab switches `active_tab` and renders the corresponding `ConversationView`.
The tab strip is a simple `h_flex` of `Tab` components — the same component
used in the agent panel's toolbar and settings pages.

### 4. Wire per-tab system prompt

The `CuratorAgentServer` already injects curator context via
`static_context`. The per-tab system prompt (which MCP server's tools are
in scope, what the server does) should be injected the same way. The
`CuratorAgentServer` (or a thin wrapper) reads the active tab's server and
builds the system prompt.

### 5. Wire per-tab tool scope

The `NativeAgent` already has a `ToolRouter` that filters MCP tools. The
kask panel's tab strip sets the active server, and the `ToolRouter` is
configured to only expose that server's tools for that tab's thread.

## Key constraints

- The kask panel is a center-pane `Item` (not a dock `Panel`). This is
  correct and should be preserved — it matches `TerminalView`.
- The `Toggle`/`ToggleFocus` actions and `KaskPanelButton` should be
  preserved (they're the entry points).
- `KanbanBoardView`, `PortfolioDashboardView`, `ScenariosView` are
  separate center-pane items — don't touch them.
- The kask panel must look and behave identically to the agent panel
  (same rendering, same interaction patterns). The only visible difference
  is the tab strip at the top.
- Follow the `.rules` file: no `unwrap()`, no `let _ =` on fallible
  operations, no `mod.rs` files, prefer existing files.
- The `ConversationView` constructor needs `Agent::Curator.server(...)` —
  check `crates/agent_ui/src/agent_ui.rs` for the `Agent::server()` method.
- The `ConversationView` needs a `connection_store: Entity<AgentConnectionStore>`
  — check how the agent panel creates this.
- The `ConversationView` may need the `thread_store` — check if the
  kask panel needs thread persistence (probably not for v1).

## What to check after implementation

1. `cargo check -p kask_panel` — compiles clean
2. `cargo clippy -p kask_panel` — no warnings
3. The panel opens via `kask_panel::Toggle` action
4. The panel shows a tab strip with 10 MCP server tabs
5. Clicking a tab shows a `ConversationView` (the same UI as the agent panel)
6. Text in messages is selectable and copyable
7. The input editor has the same behavior as the agent panel's
8. The panel looks visually identical to the agent panel (minus the tab strip)
