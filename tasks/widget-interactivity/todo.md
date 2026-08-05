# Widget-Layer Sovereignty — Task Checklist

Companion to `plan.md`. Grouped by phase. Check a box only when its acceptance criteria and verification pass.

## Phase 0 — Foundation & hygiene  ✅

- [x] **T0 — Relocate `ToolInvoker` to leaf crate `hkask-tool-invoker`** (M)  ✅
  - [x] New `crates/hkask-tool-invoker` with trait + `set_tool_invoker`/`shared_tool_invoker` + `BlockProvenance` moved from `swarm_panel/src/tool_invoker.rs`
  - [x] `swarm_panel` re-exports from leaf; 20+ call sites compile unchanged
  - [x] `main.rs` `PanelToolInvoker` impl resolves via re-export; trait body byte-identical
  - [x] Workspace `Cargo.toml` gains `hkask-tool-invoker` (member + workspace dep)
  - [x] `cargo check` clean; 4 leaf-crate provenance tests pass
- [x] **T1 — Delete dead `EventEmitter<TransportEvent> for MediaWidget`** (XS)  ✅
  - [x] Removed `crates/hkask-media-widget/src/media_widget.rs:404` + unused `EventEmitter` import
  - [x] `cargo check -p hkask-media-widget` clean
- [x] **CHECKPOINT C1** — workspace builds, clippy clean, widget + swarm-panel tests pass; relocation complete  ✅

## Phase 1 — Track 1: close the dispatch gap (Slice E)  ✅

- [x] **T2 — Wire scenarios `Next:`/`PIPELINE_STAGES` rungs to dispatch** (S)  ✅
  - [x] `on_click` on the `Next:` label + ladder rungs → `shared_tool_invoker().invoke_tool("hkask-mcp-scenarios", <tool>, <args>)`
  - [x] Pending/error state surfaced (`dispatch_in_flight`, `dispatch_error`, `dispatch_result`); missing invoker shows a visible error
  - [x] Pure `build_dispatch_args` helper unit-tested; serialized `MockToolInvoker` GPUI integration test
  - [x] `cargo test -p hkask-scenarios-widget` passes

## Phase 2 — Track 2: provenance + fan-out  ✅

- [x] **T3 — Add `BlockProvenance` + scenarios field + `hkask-mcp-scenarios` emits it** (M)  ✅
  - [x] `BlockProvenance { tool, server, args, span_id }` in `hkask-tool-invoker` (done with T0 foundation)
  - [x] `ScenariosBlockBody` gains `provenance`; `scenario_status` bakes in server-authoritative provenance
  - [x] Existing `scaffolding_*` tests still pass
- [x] **T4 — Scenarios provenance-driven dispatch** (S)  ✅
  - [x] `on_click` merges rung override into `self.body.provenance.args`, dispatches with `provenance.server`
  - [x] Falls back to T2 defaults when provenance absent; mismatch surfaces visible hint
- [x] **CHECKPOINT C2** — end-to-end scenarios dispatch works  ✅
- [x] **T5 — Portfolio fan-out: provenance + date-scrub → `portfolio_returns` (fixes SF-4)** (M)  ✅
  - [x] `provenance` on `PortfolioBlockBody`; `hkask-mcp-companies` emits it
  - [x] Date-range scrub affordance (editable date chips + Apply) → dispatch `portfolio_returns`
  - [x] **SF-4 fixed:** `parse_date_arg` returns `invalid_argument` on malformed dates; all `unwrap_or_default()` on dates removed (`tools/portfolio.rs:181,346,371`)
  - [x] `cargo test -p hkask-portfolio-widget -p hkask-mcp-companies` passes (167 + 25)
- [x] **T6 — Kanban fan-out: provenance + card affordance → `kanban_task_move`** (M)  ✅
  - [x] `provenance` on `KanbanBlockBody` (additive, `#[serde(default)]`); per-card move affordance → dispatch `kanban_task_move` via the empty-provenance fallback (`DEFAULT_SERVER`)
  - [x] Per-card cycling status-chip move affordance → dispatch `kanban_task_move` (confirmed arg shape `{task_id, target_status}`)
  - [x] Missing invoker → warning banner; partial provenance → "provenance incomplete" hint; single-flight guard
  - [x] `cargo test -p hkask-kanban-widget -p hkask-mcp-kata-kanban` passes (25 + 44/43/22)
  - [x] **D2 fix (adversarial review):** deleted dead `build_kanban_block_body` + constants + tests (0 production call sites; the fallback is the real path)
- [x] **CHECKPOINT C3** — three widgets dispatch through the governed path (scenarios D1-fixed: rung tool authoritative, provenance server-only)  ✅

## Phase 2.6 — Adversarial-review fixes  ✅

- [x] **D1 (critical): scenarios rung dispatch** — deleted the `provenance_tool == rung_tool` mismatch guard; rung tool authoritative, provenance contributes only `server`; removed `merge_args` + `PROVENANCE_MISMATCH_MSG`; added production-shape regression tests (`dispatch_args_status_block_rung_dispatches_not_mismatch`, GPUI `dispatch_rung_routes_rung_for_server_produced_block`)  ✅
- [x] **Reask "gate" → "tap" relabel** (honesty) — doc comments + dropped the `let _reask` binding in `kask_bridge`  ✅
- [x] **M1:** dropped unused `log` dep from `hkask-tool-invoker`  ✅
- [x] **M2:** `record_render`/`correlate_reask` emit `tracing::warn!` on mutex poison (no silent telemetry loss)  ✅
- [x] **M3:** `audio_load_task: Option<Task<()>>` single-flight guard on `load_audio_file_async` (cancels stale reads + drop-mid-read I/O waste)  ✅
- [x] **M6:** deduped `is_empty_provenance` → `BlockProvenance::is_empty()` in the leaf  ✅
- [x] **M5:** inlined the `swarm_panel/src/tool_invoker.rs` 1-line `pub use` shim into `swarm_panel.rs`; deleted the dead-weight file (essentialist)  ✅
- [x] **M7:** scenarios `dispatch_result` now unwraps the `{"content":…}` envelope via `hkask_types::tool_response::parse_tool_response` (the `.rules` single seam) before display  ✅

## Phase 2.5 — Media playback hygiene (SF-2/SF-3)  ✅

- [x] **SF-2/SF-3 — Stop perpetual 30fps re-render of idle media widgets**  ✅
  - [x] `TransportState` derives `PartialEq`; `MediaWidget` gains `last_transport: Option<TransportState>`
  - [x] `tick_playback` returns `bool`: gates `set_state` + `cx.notify()` on transport-state change or new video frame; returns false when no player loaded
  - [x] `start_playback_loop` stops when `tick_playback` returns false
  - [x] `cargo clippy -p hkask-media-widget --all-targets` clean; `hkask-viz-core`/`agent_ui` still build
  - [x] SF-1 (foreground `std::fs::read` + decode) fixed — file read offloaded to `cx.background_spawn` via `load_audio_file_async` + pure `read_audio_file`; `play_bytes` (rodio device + decode) stays on the foreground thread; `audio_loading` flows into the transport bar's `is_loading`  ✅
  - [x] Data-URI path stays sync (in-memory base64 + decode, no file I/O)

## Phase 3 — Track 3: branch/compare (parallel, measurement-gated) — *graph widget's what-if-branching track*

- [x] **T7a — `whatif_discarded` signal (graph-widget-local)** (S)  ✅
  - [x] `reg.widget.graph_render` emitted on `GraphWidget::new` (denominator)
  - [x] `reg.widget.evidence_set` emitted on `set_evidence` (what-if started)
  - [x] `reg.widget.whatif_discarded` emitted on `Drop` with non-empty evidence (what-if lost; no branch-save yet)
  - [x] `tracing` dep added to `hkask-graph-widget`; discard rate computable from tracing-target logs
  - [x] `cargo test -p hkask-graph-widget` (12) passes; clippy clean; `hkask-viz-core`/`agent_ui` build
- [x] **T7b — `reg.widget.reask` correlator (kask-side, via D6 memory port)** (M)  ✅
  - [x] Leaf `hkask-tool-invoker`: `RenderRecord` + `record_render(tool, span_id)` + `correlate_reask(user_message) -> bool` (drains global renders, manages global prev-had-render flag, emits `reg.widget.reask` tracing span when a user-message turn follows a render turn)
  - [x] Scenarios/portfolio/kanban widgets: `record_render` + `reg.widget.render` tracing emit on construction (provenance-carrying widgets only; graph is measured by `whatif_discarded`)
  - [x] `kask_bridge::BridgeMemoryPort::ingest_turn`: calls `correlate_reask(!user_input.trim().is_empty())` on each completed turn (D6 hook — no upstream edits)
  - [x] Coarse upper-bound proxy (any user message after a render turn counts; intent-matching heuristic remains open question #3; global flag → multi-conversation noise, acceptable for the aggregate gate)
  - [x] `cargo test` 7+75+124 pass; clippy clean across 5 crates; `hkask-viz-core`/`agent_ui` build
- [x] **Gate (T7)** documented: >15% re-ask **or** >5% what-if-discarded → proceed to T8a; <5% both → defer Track 3 + Phase 4; in between → human (both halves now measurable: reask via T7b, what-if-discarded via T7a)  ✅
- [x] **T8 - Graph widget what-if branch/revert/compare (widget-internal)** (M)  DONE
  - Design decision (essentialist): the plan T8a cache typed-handle + version-key rewrite was SKIPPED as over-built. Branches are widget-local state (evidence snapshots); the cache already maps body to entity (the entity holds its branches); the cache rewrite would risk the shared cache all widgets depend on for a single consumer (.rules: do not build the abstraction before the second consumer). Cross-turn branch persistence does not make sense (a new agent tree has a different body/node-ids, so an old what-if evidence cannot apply). So Track 3 value is within-view branch/revert/compare, done widget-internally. The cache collision Nit (N-3) is left as documented (low-prob, not worth risking the shared cache).
  - [x] WhatIfBranch { name, evidence } + branches: Vec<WhatIfBranch> + compare_branch: Option<usize> on GraphWidget (view.rs:38-69)
  - [x] save_branch (no-op when evidence empty), revert_to_base, load_branch, delete_branch (adjusts compare_branch index), toggle_compare - all bounds-checked, no panicking indexing (view.rs:140-200)
  - [x] Controls row: Save what-if / Revert to base (shown when evidence non-empty) + per-branch Load/Compare/delete chips (view.rs:573-639)
  - [x] Compare diff panel: recomputes base vs branch marginals fresh each render via recompute_marginals; lists per node base pct to branch pct with delta (view.rs:641-691). The diff list IS the compare view (side-by-side canvases explicitly out of scope as gold-plating)
  - [x] 8 new gpui::test branch tests (save/revert/load/delete-index-adjust/toggle/out-of-bounds-noop) + existing 12 = 20 pass; script/clippy clean; hkask-viz-core/agent_ui build; impl Drop (T7a) untouched
- [x] **CHECKPOINT C4** - branch/compare/revert works end-to-end (graph widget)  DONE
- [ ] **T8a cache rewrite - deliberately deferred** (see design decision above; revisit only if a second widget needs cross-widget branching, which would justify touching the shared cache)

## Phase 4 — Downstream sovereignty patterns

- [x] **H — consent-gated side effects (kanban move)** DONE — `PendingMove` stage → Confirm/Cancel banner → `dispatch_move`; chips disabled while pending; 6 new gpui tests (31 total pass); clippy clean; kanban-widget only.
- [x] **C — "I disagree" in-place correction gesture (portfolio MVP)** — DONE. The compose-back seam is built (D21: `hkask-conversation-injector` leaf trait + `ThreadConversationInjector` prefill in `agent_ui` + `publish_injector` on activation + DIVERGENCE.md + tests). The portfolio consumer is built (`compose_disagree_body` + `on_disagree_click` → `shared_injector().inject(body, window, cx)`, surfaces a `disagree_draft` fallback when no injector, emits `reg.widget.disagree`; 3 disagree tests pass). A refactor-architecture review confirmed D21's PREFILL approach is the sound design — the auto-send variant (calling `Thread::send` directly) was rejected as flaky (it bypasses `ThreadView::send_content`'s turn-tracking/generation-indicator/telemetry); running the review before building prevented that. Fixed broken portfolio tests (VisualTestContext::update 2-arg closure arity) left by the parallel build. Other 3 widgets (scenarios/kanban/graph) do NOT yet have the consumer — follow-up (identical pattern).
- [ ] **F — inline drill-down** — "explain" half unblocked (pure dispatch on the existing ToolInvoker path); "investigate" half now unblocked via the D21 compose-back seam.
- [ ] **D — ghost edits** — unblocked via the D21 compose-back seam + an "evaluate, don't execute" turn framing.
- [ ] **C on the other 3 widgets** (scenarios/kanban/graph) — follow-up (identical to the portfolio consumer).
- [ ] **I — ontology-bounded affordances** — deferred until a 2nd ontology domain (per .rules, don't pre-build the catalog trait).