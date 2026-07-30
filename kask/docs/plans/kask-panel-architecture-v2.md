---
title: "Kask Panel Architecture v2 — Superseded"
audience: [zed-kask integrators, hKask architects]
last_updated: 2026-07-29
version: "0.4.0"
status: "Superseded"
domain: "composition"
mds_categories: [composition, trust, lifecycle, curation]
---

# Kask Panel Architecture v2 — Superseded

> **Status: Superseded.** This plan is fully implemented. The kask panel is
> now a thin `ConversationView` wrapper (one tab per built-in MCP server,
> each lazily constructing a `ConversationView` with `Agent::Curator` and a
> per-tab system prompt via `CuratorAgentServer::with_extra_static_context`).
> The current authoritative reference is the Diataxis documentation set:
> [`diataxis/kask_panel/`](../diataxis/kask_panel/).

## What this plan proposed

The plan (v0.3.0) proposed replacing the kask panel's ~1400 lines of custom
rendering code with a thin wrapper around the agent panel's
`ConversationView`. The panel would host one `ConversationView` per MCP
server tab, with a tab strip for context switching, and inject the per-tab
system prompt via `static_context`.

## What was implemented

All of it. The implemented panel matches the plan's Option A:

- `KaskPanel` (`crates/kask_panel/src/kask_panel.rs:168`) is a center-pane
  `Item` holding `HashMap<usize, Entity<ConversationView>>` — one retained
  `ConversationView` per tab.
- `ensure_thread_for_tab` (`kask_panel.rs:224`) lazily constructs each tab's
  `ConversationView` with `Agent::Curator` and
  `CuratorAgentServer::with_extra_static_context(per_tab_system_prompt(server))`.
- The `ConversationView` handles all rendering (messages, input, tool-call
  cards, scroll, retry, cancel, copy, markdown, streaming, mentions,
  drag-and-drop). The kask panel only adds the tab strip.
- `init` (`kask_panel.rs:447`) registers `Toggle` (deploys new item) and
  `ToggleFocus` (focus-only) actions, per the `.rules` "Center-pane Item
  Toggle vs ToggleFocus" trap. The `Toggle` handler calls
  `panel.focus_handle(cx).focus(window, cx)` after
  `add_item_to_active_pane`, per the `.rules` "Center-pane Item
  deploy-and-focus" trap.
- The `ToolInvoker` trait (`kask_panel.rs:89`) + `set_tool_invoker` hook
  (`kask_panel.rs:106`) remain for the visualization views
  (`KanbanBoardView`, `PortfolioDashboardView`, `ScenariosView`), which fetch
  data via direct MCP tool calls. The chat panel itself routes through
  `NativeAgent`'s `ToolRouter`.
- There is no `ScopedInference` trait, `RegulationStatus` trait, or
  `RegulationSnapshot` struct in this crate. The structural pins
  `kask_panel_has_no_curator_session_trait` and
  `kask_panel_has_no_regulation_status_bar` assert this.

## Why this doc is now a stub

Per the DOCUMENTATION_STANDARDS lifecycle (Superseded → Removed), a plan
that is fully implemented and whose current state is covered by the
Diataxis reference docs no longer earns its place as an active plan. The
Diataxis set is the canonical reference:

- [`diataxis/kask_panel/reference.md`](../diataxis/kask_panel/reference.md) —
  class diagram, source citations, panel hooks, sub-views.
- [`diataxis/kask_panel/explanation.md`](../diataxis/kask_panel/explanation.md)
  — why a thin `ConversationView` wrapper, why the curator is the agent for
  every tab, why `ToolInvoker` is separate from the chat path.
- [`diataxis/kask_panel/how-to.md`](../diataxis/kask_panel/how-to.md) —
  adding a new panel action.
- [`diataxis/kask_panel/tutorial.md`](../diataxis/kask_panel/tutorial.md) —
  your first panel action.

**Flagged for human deletion decision:** this stub can be deleted once the
team confirms the Diataxis docs are sufficient. It is kept as a stub (not
deleted inline) so the audit trail of "what was planned vs what was built"
is preserved for one review cycle.

---

[^gpui]: Zed Industries. (2024). *GPUI — Zed's GPU-accelerated UI framework.* <https://github.com/zed-industries/zed>.
