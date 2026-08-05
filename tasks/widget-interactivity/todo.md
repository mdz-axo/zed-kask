# Widget-Layer Sovereignty — Task Checklist

Companion to `plan.md`. Grouped by phase. Check a box only when its acceptance criteria and verification pass.

## Phase 0 — Foundation & hygiene

- [ ] **T0 — Relocate `ToolInvoker` to leaf crate `hkask-tool-invoker`** (M)
  - [ ] New `crates/hkask-tool-invoker` with trait + `set_tool_invoker`/`shared_tool_invoker` moved verbatim from `swarm_panel/src/tool_invoker.rs`
  - [ ] `swarm_panel` re-exports from leaf; 12+ call sites compile unchanged
  - [ ] `main.rs` `PanelToolInvoker` import + `set_tool_invoker` call updated; trait body byte-identical
  - [ ] Workspace `Cargo.toml` gains `hkask-tool-invoker`
  - [ ] `cargo check -p hkask-tool-invoker -p swarm_panel -p zed` + `./script/clippy` clean; a swarm-panel dispatch test still passes
- [ ] **T1 — Delete dead `EventEmitter<TransportEvent> for MediaWidget`** (XS)
  - [ ] Remove `crates/hkask-media-widget/src/media_widget.rs:404`
  - [ ] `cargo check -p hkask-media-widget` + clippy clean; grep confirms absence
- [ ] **CHECKPOINT C1** — workspace builds, clippy clean, widget + swarm-panel tests pass; human reviews the relocation

## Phase 1 — Track 1: close the dispatch gap (Slice E)

- [ ] **T2 — Wire scenarios `Next:`/`PIPELINE_STAGES` rungs to dispatch** (S)
  - [ ] `on_click` on the `Next:` label + ladder rungs → `shared_tool_invoker().invoke_tool("hkask-mcp-scenarios", <tool>, <args>)`
  - [ ] Pending/error state surfaced (`dispatch_in_flight`, `dispatch_error`); missing invoker shows a visible error, not a silent no-op
  - [ ] `MockToolInvoker` test asserts `("hkask-mcp-scenarios", "scenario_frame", <args>)` received
  - [ ] `cargo test -p hkask-scenarios-widget` + clippy pass; manual click on a ```` ```scenarios ```` block

## Phase 2 — Track 2: provenance + fan-out

- [ ] **T3 — Add `BlockProvenance` + scenarios field + `hkask-mcp-scenarios` emits it** (M)
  - [ ] `BlockProvenance { tool, server, args, span_id }` in `hkask-tool-invoker` (all `#[serde(default)]`)
  - [ ] `ScenariosBlockBody` gains `provenance` field; parses with and without (defaults empty)
  - [ ] `hkask-mcp-scenarios` emits a block body with non-empty `provenance` for `scenario_status` (server bakes it in — open question 1)
  - [ ] Existing `scaffolding_*` tests still pass; `cargo test -p hkask-scenarios-widget -p hkask-mcp-scenarios` + clippy
- [ ] **T4 — Scenarios provenance-driven dispatch** (S)
  - [ ] `on_click` merges the rung's parameter change into `self.body.provenance.args` and dispatches with `provenance.server` when present
  - [ ] Falls back to T2 defaults when provenance absent; fabricated/missing provenance surfaced, not silently dropped
  - [ ] Test: body with `provenance.tool="scenario_quantify"` dispatches merged args; `cargo test -p hkask-scenarios-widget`
- [ ] **CHECKPOINT C2** — end-to-end scenarios dispatch works; human reviews the provenance contract
- [ ] **T5 — Portfolio fan-out: provenance + date-scrub → `portfolio_returns` (fixes SF-4)** (M)
  - [ ] `provenance` on `PortfolioBlockBody`; `hkask-mcp-companies` emits it
  - [ ] Date-range scrub affordance on the Returns row → dispatch `portfolio_returns` with modified `from`/`to`
  - [ ] **SF-4 fix:** validate `from`/`to` (`tools/portfolio.rs:331-361`) → `McpToolError::invalid_argument` on parse failure, not `unwrap_or_default()` to 1970-01-01
  - [ ] `cargo test -p hkask-portfolio-widget -p hkask-mcp-companies` + clippy
- [ ] **T6 — Kanban fan-out: provenance + card affordance → `kanban_task_move`** (M)
  - [ ] `provenance` on `KanbanBlockBody`; `hkask-mcp-kata-kanban` emits it
  - [ ] Card affordance dispatches `kanban_task_move` with `{task_id, status}` + `provenance.server`
  - [ ] Missing invoker surfaces error; provenance-absent disables affordance with "ask the agent" hint
  - [ ] `cargo test -p hkask-kanban-widget -p hkask-mcp-kata-kanban` + clippy
- [ ] **CHECKPOINT C3** — three widgets dispatch through the governed path; human reviews the fan-out pattern

## Phase 3 — Track 3: branch/compare (parallel, measurement-gated)

- [ ] **T7 — Measurement gate: `reg.widget.reask` span** (S)
  - [ ] Span emitted, keyed by `provenance.span_id`, on re-ask detection; aggregate rate computable
  - [ ] Gate decision documented: >15% → proceed; <5% → defer Track 3 + Phase 4; 5–15% → human
- [ ] **T8a — Cache typed-handle + version key** (M) — *only if T7 gate >15%*
  - [ ] `CachedWidget` retains typed dispatch handle / version discriminator; `cache_key` includes version id + body-equality check on hit
  - [ ] Two branches coexist; collision no longer returns wrong widget; `cache_key_is_stable` + `viz_factories_cover_four_widgets` updated and pass
  - [ ] `cargo test -p hkask-viz-core` + clippy
- [ ] **T8b — Branch/revert UI** (S) — *only if T8a*
  - [ ] "Keep / try different assumption" creates a branch; "Revert" restores agent's version
- [ ] **T8c — Compare side-by-side UI** (S) — *only if T8a*
  - [ ] Two cached widgets render simultaneously
- [ ] **CHECKPOINT C4** — branch/compare/revert works end-to-end

## Phase 4 — Deferred downstream (gated on T7)

- [ ] C — "I disagree" in-place correction gesture (downstream of T4, gated on T7)
- [ ] F — inline drill-down "explain"/"investigate" (downstream of T4, gated on T7)
- [ ] D / H — ghost edits / consent-gated side effects (policy on top of dispatch; not architectural)
- [ ] I — ontology-bounded affordances (config of which buttons dispatch exposes; real seam only on 2nd ontology domain)