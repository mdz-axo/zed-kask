# TUI Architecture

**Last updated:** 2026-07-23 · **Version:** v0.31.0

## Overview

The hKask TUI is a multi-window, split-pane terminal interface modelled on
Zed's workspace architecture. A binary tree of splits hosts stateful `Window`
implementations, with keyboard-driven focus, split, close, and tab management.
Windows are opened via slash commands (`/open kanban`) or keybindings
(`Ctrl-W` prefix sub-mode).

## Architecture

```text
TuiSession
  └── Workspace
        ├── Tab bar (rendered only if >1 tab)
        ├── SplitNode tree (binary splits)
        │     ├── Leaf: Box<dyn Window>
        │     ├── Horizontal { left, right, ratio }
        │     └── Vertical   { top, bottom, ratio }
        └── StatusBar (global, 1 line)
```

### Key files

| File | Role |
|---|---|
| `crates/hkask-repl/src/tui/mod.rs` | `TuiSession` — owns terminal, event loop, layout restore/save |
| `crates/hkask-repl/src/tui/workspace.rs` | `Workspace` + `SplitNode` tree — split, close, focus, tab ops |
| `crates/hkask-repl/src/tui/window.rs` | `Window` trait, `WindowKind`, `WorkspaceAction`, `SplitDirection` |
| `crates/hkask-repl/src/tui/window_catalog.rs` | `create_window()` factory — maps `WindowKind` to concrete impl |
| `crates/hkask-repl/src/tui/repl_bridge.rs` | Bridge traits: `SystemBridge`, `ReplBridge`, `SettingsBridge`, `SessionBridge`, `ToolInvokeBridge` |
| `crates/hkask-repl/src/tui/layout.rs` | `SavedLayout`/`SavedSplit`/`SavedTab` JSON persistence |
| `crates/hkask-repl/src/tui/tab.rs` | `Tab { name, root: SplitNode }` |
| `crates/hkask-repl/src/tui/status_bar.rs` | `StatusBar` — model, gas, Regulation, context |
| `crates/hkask-repl/src/tui/windows/chat.rs` | `ChatWindow` — primary AI interaction surface |
| `crates/hkask-repl/src/tui/windows/mcp_scoped.rs` | `McpScopedWindow` + `McpScopedState` — shared base for all MCP-backed windows |

## Window kinds

4 window kinds are implemented:

| Kind | Title | MCP Server | Singleton | Description |
|---|---|---|---|---|
| `Chat` | Chat | — | No (multiple allowed) | AI chat with inference + curator modes |
| `Kanban` | Kanban | `kanban` | Yes | Task coordination via `hkask-mcp-kata-kanban` |
| `Companies` | Companies | `companies` | Yes | Financial data via `hkask-mcp-companies` |
| `Scenarios` | Scenarios | `scenarios` | Yes | Scenario planning via `hkask-mcp-scenarios` |

`WindowKind` uses direct `match` expressions for `default_title`, `description`,
`allows_multiple`, and `parse_kind` — the compiler enforces exhaustiveness when
new variants are added.

### Adding a new MCP window

1. Add a `WindowKind` variant + match arms (~6 lines in `window.rs`)
2. Add a factory arm in `window_catalog.rs` using `McpScopedWindow::new(...)` (~8 lines)
3. No new files needed — `McpScopedWindow` is the generic window for any MCP server

The remaining 12 MCP servers (memory, research, communication, media, docproc,
training, replica, skill, filesystem, codegraph, regulation, condenser) can each
get a window by following this 14-line pattern.

## MCP window architecture

`McpScopedWindow` is a single concrete `Window` impl backed by `McpScopedState`.
It supports two input paths:

1. **Direct tool invocation** (`:tool_name args`) — calls the MCP tool directly
   via `ToolInvokeBridge`, bypassing the LLM. Fast, structured JSON results.
   Preserves OCAP governance (DelegationToken), gas accounting, Regulation spans.
2. **Scoped inference** (natural language) — runs inference scoped to the MCP
   server's tools via `start_scoped_inference`. The LLM acts as an intermediary
   that calls the appropriate MCP tools.

The `ToolInvokeBridge` trait is separate from `ReplBridge` (keeping each ≤7
items). The implementation calls `McpRuntime::invoke(server, tool, args, &token)`
through the same governance membrane as the inference loop. The async start/poll
pattern (`start_mcp_tool_invoke` / `poll_mcp_tool_invoke`) mirrors
`start_inference` / `poll_inference` to avoid blocking the TUI event loop.

## Bridge traits

| Trait | Surface | Used by |
|---|---|---|
| `SystemBridge` | 9 methods (read-only monitoring) | `Workspace` tick |
| `ReplBridge` | 6 methods (inference + monitoring) | `ChatWindow`, `McpScopedWindow` |
| `SettingsBridge` | 4 methods (model/settings mutation) | `ChatWindow` (optional) |
| `SessionBridge` | 3 methods (agent/session state) | `ChatWindow` (optional) |
| `ToolInvokeBridge` | 2 methods (direct MCP tool calls) | `McpScopedWindow` (optional) |

All traits are implemented by `TuiReplBridge` in `crates/hkask-repl/src/lib.rs`.
Test mocks are in `crates/hkask-repl/src/tui/test_util.rs`.

## Keybindings

| Key | Action |
|---|---|
| `Ctrl+Q` | Quit |
| `Ctrl+W v` | Split vertical (side-by-side) |
| `Ctrl+W s` | Split horizontal (stacked) |
| `Ctrl+W c` | Close focused pane |
| `Ctrl+W w` | Cycle focus next |
| `Ctrl+W p` | Cycle focus prev |
| `Ctrl+T` | New tab |
| `Ctrl+Tab` | Next tab |
| `Ctrl+Shift+Tab` | Previous tab |

## Slash commands (TUI-only)

| Command | Action |
|---|---|
| `/open <kind>` | Open window as split from focused |
| `/close` | Close focused window |
| `/split h\|v` | Split focused window |
| `/focus` | Cycle focus next |
| `/tab new [name]` | Create new tab |
| `/tab next\|prev` | Switch tabs |

In MCP windows, `:tool_name args` invokes a tool directly; all other non-slash
input goes to scoped LLM inference.

## Layout persistence

Layouts are saved per-userpod to `~/.config/hkask/userpods/<name>/tui_layout.json`.
The `SavedLayout` structure stores tabs, split trees (with ratios), and window
kinds by title string. On restore, `WindowKind::parse_kind` maps titles back to
enum variants. Unknown kinds fall back to `Chat` (defense-in-depth; `is_valid`
rejects unknown kinds before this fallback is reached).

## WorkspaceAction flow

Windows emit `WorkspaceAction` values via `drain_actions()` (returning `Vec` for
multi-action-per-tick support). The `Workspace::tick()` method collects all
actions, then dispatches them through `apply_action()`. This separation ensures
windows cannot mutate the split tree directly — they request structural changes
through the action channel, and the workspace executes them.

## Design decisions

- **`Leaf(Box<dyn Window>)`** — not `Leaf(Option<...>)`. Eliminates `unreachable!`
  guards. Requires a `PlaceholderWindow` during tree surgery (one synchronous
  line, never observed by render/tick/layout).
- **By-value tree ops** (`replace_leaf_with_split`, `remove_window` take `self`)
  — functional-persistent style, necessary because `&mut self` cannot move out
  of `&mut T` without a replacement value.
- **`McpScopedWindow` as single generic type** — no per-server window files.
  Adding a new MCP window is 14 lines (enum variant + factory arm).
- **`McpInvokeError` structured enum** — not `String`. Maps `ToolPortError`
  variants to `ToolNotFound` / `Server`. Aligns with the project's
  `No Result<_, String>` CI gate.
- **No directional focus (Ctrl-W h/j/k/l)** — cycle focus is sufficient for the
  realistic pane count (2-4 panes in a terminal). Directional focus would add
  ~80 lines + a render-sidecar map for marginal value. Fails the essentialist
  deletion test.