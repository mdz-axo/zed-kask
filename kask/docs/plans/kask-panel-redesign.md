---
title: "Kask Panel Redesign (v3) — Superseded"
audience: [zed-kask integrators, hKask architects, GPUI engineers]
last_updated: 2026-07-29
version: "0.4.0"
status: "Superseded"
domain: "composition"
mds_categories: [composition, trust, lifecycle, curation]
---

# Kask Panel Redesign (v3) — Superseded

> **Status: Superseded.** This plan (v3) proposed growing the kask panel in
> place with a custom `CuratorSession` trait, `ToolScope` enum,
> `CuratorEvent` stream, `StreamingMessage` model, and `RegulationSnapshot`
> status bar. **That path was not taken.** The implemented panel instead
> reuses the agent panel's `ConversationView` directly (the v2 plan's
> Option A), which the v3 doc had argued against. The current authoritative
> reference is the Diataxis documentation set:
> [`diataxis/kask_panel/`](../diataxis/kask_panel/).

## What this plan proposed (v3, Option B)

The v3 plan argued that the v2 plan's Option A (thin `ConversationView`
wrapper) was wrong, and proposed Option B: keep the kask panel as a
purpose-built `Item` and grow it minimally with:

- A `CuratorSession` trait (per-tab, stateful, streaming, tool-scoped) —
  ~150 lines.
- A `ToolScope::Server` enum for per-tab tool filtering.
- A `CuratorEvent` stream (`TextDelta`, `ThinkingDelta`, `ToolCall`,
  `ToolResult`, `Done`, `Error`).
- A `StreamingMessage` model and `TabState` per tab.
- Markdown rendering via the `markdown` crate (not `ThreadView`).
- A `RegulationSnapshot` status bar.

## What was actually implemented (v2 Option A won)

The implemented panel does NOT contain any of the v3-proposed abstractions.
The structural pin tests `kask_panel_has_no_curator_session_trait` and
`kask_panel_has_no_regulation_status_bar` (in `kask_panel.rs` tests) assert
that no `CuratorSession` trait, `RegulationSnapshot` struct, or
`RegulationStatus` trait exists in the crate. Instead:

- `KaskPanel` (`crates/kask_panel/src/kask_panel.rs:168`) hosts one
  `ConversationView` per tab (`HashMap<usize, Entity<ConversationView>>`).
- Each tab's `ConversationView` is constructed with `Agent::Curator` and a
  per-tab system prompt via `CuratorAgentServer::with_extra_static_context`
  (`kask_panel.rs:224`).
- The `ConversationView` handles all rendering, streaming, tool-call cards,
  retry, cancel, copy, markdown, mentions, drag-and-drop.
- The `ToolInvoker` trait (`kask_panel.rs:89`) remains for the visualization
  views (`KanbanBoardView`, `PortfolioDashboardView`, `ScenariosView`), not
  the chat path.

The v3 doc's cybernetic and essentialist arguments against the fork were
correct in identifying that the v2 *fork* (29k-line `ThreadView` copy) was
wrong. But the resolution was not "grow the panel in place" (v3 Option B) —
it was "reuse `ConversationView` without forking" (v2 Option A, which v3
had argued against). The v3-proposed abstractions would have re-implemented
what `ConversationView` already provides.

## Why this doc is now a stub

Per the essentialist deletion test, a plan whose proposed abstractions were
deliberately NOT implemented (and are structurally pinned as absent) earns
no place as an active plan. Keeping it active would mislead a future reader
into thinking `CuratorSession`/`ToolScope`/`RegulationSnapshot` exist or
are planned. The Diataxis docs are the canonical reference for what was
actually built.

**Flagged for human deletion decision:** this stub can be deleted once the
team confirms the Diataxis docs are sufficient. It is kept as a stub so the
audit trail of "v3 proposed X, the team chose Y, here's why" is preserved
for one review cycle.

---

[^gpui]: Zed Industries. (2024). *GPUI — Zed's GPU-accelerated UI framework.* <https://github.com/zed-industries/zed>.
