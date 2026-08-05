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
  - [x] `provenance` on `KanbanBlockBody`; `build_kanban_block_body` bakes it in
  - [x] Per-card cycling status-chip move affordance → dispatch `kanban_task_move` (confirmed arg shape `{task_id, target_status}`)
  - [x] Missing invoker → warning banner; partial provenance → "provenance incomplete" hint; single-flight guard
  - [x] `cargo test -p hkask-kanban-widget -p hkask-mcp-kata-kanban` passes (25 + 46/43/22)
- [x] **CHECKPOINT C3** — three widgets dispatch through the governed path  ✅

## Phase 2.5 — Media playback hygiene (SF-2/SF-3)  ✅

- [x] **SF-2/SF-3 — Stop perpetual 30fps re-render of idle media widgets**  ✅
  - [x] `TransportState` derives `PartialEq`; `MediaWidget` gains `last_transport: Option<TransportState>`
  - [x] `tick_playback` returns `bool`: gates `set_state` + `cx.notify()` on transport-state change or new video frame; returns false when no player loaded
  - [x] `start_playback_loop` stops when `tick_playback` returns false
  - [x] `cargo clippy -p hkask-media-widget --all-targets` clean; `hkask-viz-core`/`agent_ui` still build
  - [ ] SF-1 (foreground `std::fs::read` + decode) remains open — offload file read to `cx.background_spawn`, keep `play_bytes` on foreground (separate follow-up)

## Phase 3 — Track 3: branch/compare (parallel, measurement-gated) — *graph widget's what-if-branching track*

- [ ] **T7 — Measurement gate: `reg.widget.reask` + `reg.widget.whatif_discarded` spans** (S)
  - [ ] `reg.widget.reask` emitted, keyed by `provenance.span_id`, on re-ask detection; aggregate rate computable
  - [ ] `reg.widget.whatif_discarded` emitted when graph-widget evidence is set then lost (overwritten / navigated away) without saving a branch
  - [ ] Gate decision documented: >15% re-ask **or** >5% what-if-discarded → proceed to T8a; <5% both → defer Track 3 + Phase 4; in between → human
- [ ] **T8a — Cache typed-handle + version key** (M) — *only if T7 gate passes*
  - [ ] `CachedWidget` retains typed dispatch handle / version discriminator; `cache_key` includes version id + body-equality check on hit
  - [ ] Two branches coexist (e.g. graph widget's agent tree + its what-if tree); collision no longer returns wrong widget; `cache_key_is_stable` + `viz_factories_cover_four_widgets` updated and pass
  - [ ] `cargo test -p hkask-viz-core` + clippy
- [ ] **T8b — Branch/revert UI** (S) — *only if T8a*
  - [ ] "Keep / try different assumption" creates a branch; "Revert" restores agent's version; graph-widget what-if can be saved as a named branch
- [ ] **T8c — Compare side-by-side UI** (S) — *only if T8a*
  - [ ] Two cached widgets render simultaneously (agent tree next to what-if tree, marginals differing visibly)
- [ ] **CHECKPOINT C4** — branch/compare/revert works end-to-end

## Phase 4 — Deferred downstream (gated on T7)

- [ ] C — "I disagree" in-place correction gesture (downstream of T4, gated on T7)
- [ ] F — inline drill-down "explain"/"investigate" (downstream of T4, gated on T7)
- [ ] D / H — ghost edits / consent-gated side effects (policy on top of dispatch; not architectural)
- [ ] I — ontology-bounded affordances (config of which buttons dispatch exposes; real seam only on 2nd ontology domain)