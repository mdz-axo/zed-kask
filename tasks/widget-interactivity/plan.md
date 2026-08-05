# Widget-Layer Sovereignty: Implementation Plan

**Method:** task-breakdown PDCA (PLAN → DECOMPOSE → EVALUATE → QUALITY-GATE → WRITE). Vertical slices, bottom-up dependency order, high-risk-first, checkpoints between phases.
**Provenance of the plan:** synthesizes the outputs of six skills run against the widget crates and their linked MCP servers — `code-review`, `graph-audit` (semantic), `refactor-architecture`, `sequential-inquiry`, `grill-me`, `metacognition`. Each architecture decision below cites the skill finding that grounds it.
**PKO anchor:** `pko:Procedure` targeting "Bidirectional, user-sovereign kask widget layer (widget→MCP dispatch + block provenance + version surface)".
**DC+BIBO:** `dcterms:title` "Widget-Layer Sovereignty Implementation Plan"; `dcterms:creator` "zed-kask agent (task-breakdown)"; `dcterms:created` 2026-08-05; `bibo:Document`.

---

## Overview

Today the five kask GPUI widget crates (`hkask-media-widget`, `hkask-portfolio-widget`, `hkask-kanban-widget`, `hkask-scenarios-widget`, `hkask-graph-widget`) are passive read-only views over JSON bodies the agent emits into the chat stream via the D18 fenced-block seam (`hkask_viz_core::block_renderer`). The `graph-audit` semantic pass measured graph health at 0.62 and diagnosed the deficit as a one-way display pipeline with a **structural hole where the control loop should close** — the widget layer is a sensor with no effector (Conant-Ashby Good Regulator fail).

The plan closes that loop in three tracks, ordered by leverage and risk, with the cheapest proof-of-dispatch shipped first:

- **Track 1 (close the dispatch gap):** relocate the existing governed `ToolInvoker` to a leaf crate so widgets can import it without inverting dependency direction, and wire the scenarios `Next:` ladder as the first clickable dispatch surface.
- **Track 2 (provenance + fan-out):** add a `BlockProvenance { tool, server, args, span_id }` field to block bodies so widgets can re-issue the originating MCP tool with modified args; fan out across portfolio and kanban.
- **Track 3 (branch/compare — parallel, measurement-gated):** expose `VIZ_CACHE` as a version surface so a user can keep the agent's version and a local what-if side by side. The canonical driver is the graph widget: its `set_evidence`/`repropagate` is a rich local effector today, but a good what-if is throwaway. Track 3 versions it (branch/revert/compare) rather than committing it to the server (decision 10). Gated on a measured re-ask **and** what-if-discarded rate.

The plan deliberately **does not** pre-build an `VizEffector` trait or rework `CachedWidget` for Track 1/2: cached entities are already mutable through their own `cx.listener` handlers (the cache stores the live `Entity<T>`; its `on_click` handlers dispatch via the global accessor). A shared effector trait would be speculative generality until a second consumer materializes (repo `.rules` "Trait-with-one-impl" trap). Track 3 is where the cache shape genuinely needs to change, because branch/compare needs external code to retrieve typed handles — that is its independent cost, and why it is a separate track, not "policy on top" (the `refactor-architecture` pass falsified my earlier claim that G was downstream of A). The graph widget's local effector is **correctly local** and is *not* wired to the server (decision 10): instant what-if exploration is its purpose, and the sovereignty feature it needs is versioning, not a server round-trip.

## Architecture decisions

1. **New leaf crate `hkask-tool-invoker`** holds the `ToolInvoker` trait + the `shared_tool_invoker`/`set_tool_invoker` global accessor, relocated verbatim from `crates/swarm_panel/src/tool_invoker.rs`. Deps: `gpui` (`Task`), `serde`, `serde_json`. The production impl `PanelToolInvoker` stays in `crates/zed/src/main.rs` (it needs `hkask-capability` + `hkask-mcp`); it just gains a dep on the leaf crate instead of defining the trait locally. `swarm_panel` re-exports from the leaf so its 12+ call sites compile unchanged. *Ground: refactor-architecture F7 (dependency direction); the trait already has one production impl but many call sites — not speculative.*

2. **Dispatch is read via the global accessor at event time, not threaded through `VizWidget`.** No `VizWidget` trait signature change, no constructor change, no per-widget `Arc<dyn ToolInvoker>` field. The four existing impls (`hkask_viz_core.rs:108-166`) survive byte-for-byte. *Ground: refactor-architecture ra-deepen; the cheaper seam the inquiry pass identified vs. my original "widget-held handle" framing.*

3. **The OCAP membrane is already built and already crossed.** `PanelToolInvoker` mints `panel_default_token(DelegationResource::Tool, tool, Execute, webid, webid)` and calls `ToolPort::invoke` → `McpRuntime::invoke` (`main.rs:2826-2857`, `runtime.rs:507`), which enforces `is_valid_for` + gas. A widget calling `shared_tool_invoker()` reuses this governed path — no new membrane. *Ground: sequential-inquiry falsifiability pass refuted H5 (cost-exceeds-value).*

4. **Bypass-the-agent is the established pattern, not a new risk.** `swarm_panel` already issues state-mutating tool calls (`swarm_clone_to_local`, `swarm_push_to_cloud`, `swarm_remove_local`, `swarm_fire`, `swarm_add_agent_to_swarm`) via `shared_tool_invoker` concurrent with agent turns. The dual-controller hazard is already tolerated in production; the widget layer doing the same is architecturally consistent. *Ground: H4 grep this session (`swarm_panel.rs:986,1029,1231,1273,+add`). Compose-back-to-agent remains an available coherence option for the most lossy mutations, not a prerequisite.*

5. **`BlockProvenance` lives in the `hkask-tool-invoker` leaf crate** as the provenance payload that flows through dispatch (cohesive: the crate is "what widgets use to talk to MCP"). It is `#[serde(default)]` and additive on each block body, so existing tolerant parsers and the `viz_factories_cover_four_widgets` test survive. *Open question: reviewers may prefer a separate `hkask-viz-block` leaf if viz-block concerns grow; split is cheap later.*

6. **No `VizEffector` trait is introduced in Track 1/2.** Each widget dispatches in its own `cx.listener` handler against the global accessor. Extract a shared trait only when a second consumer of the abstraction materializes. *Ground: repo `.rules` "Trait-with-one-impl is speculative generality"; metacognition Architect rotation flagged the missing effector surface but the cached entity is already self-mutating, so the trait is not a prerequisite — it is a "watch for emergence" note.*

7. **Track 3 (cache rewrite) is independent and measurement-gated.** `CachedWidget` (`hkask_viz_core.rs:176-194`) is an erased `Box<dyn Fn() -> AnyElement>`; the cache key (`:275-279`) is a bare `u64` body hash with no version/branch discriminator and no collision handling. Branch/compare needs typed handles out of the cache + a version/branch key — orthogonal to provenance. Defer until Track 2 is on ≥2 widgets **and** a `reg.widget.reask` span shows >15% re-ask on widget-bearing turns. *Ground: refactor-architecture F3 (cache asymmetry: A does not need a cache change, G does).*

8. **Cheap graph-health hygiene ships in Phase 0.** Delete the dead `impl EventEmitter<TransportEvent> for MediaWidget` (`media_widget.rs:404`) — no emitter, no subscriber (grep-confirmed). *Ground: graph-audit remedy #2 + essentialist deletion test + repo `.rules` "Advertised invariants need enforcement points."*

9. **Media playback perf — SF-2/SF-3 fixed; SF-1 remains.** `code-review` flagged SF-1 (foreground audio file read + rodio decode `media_widget.rs:171/245`), SF-2 (perpetual 30fps re-render `:283-300`), SF-3 (loop fires on load-failure `:139/154`). **SF-2 and SF-3 are fixed** (this session): `tick_playback` now returns `bool` and gates `set_state` + `cx.notify()` on actual transport-state change or a new video frame; the loop stops when there is no loaded player. A paused/stopped/finished media widget no longer re-renders at 30 fps forever. **SF-1 remains open** (foreground `std::fs::read` + decode); the safe fix offloads only the file read to `cx.background_spawn`, keeping `play_bytes` (rodio device + decode) on the foreground thread — scheduled as a separate follow-up, not in this plan. SF-4 (date `unwrap_or_default` → 1970-01-01, `hkask_mcp_companies/tools/portfolio.rs:331-361`) was incidentally fixed by T5's portfolio fan-out.

10. **No evidence-commit MCP tool for the graph widget — its effector is correctly local; Track 3 versions what-ifs instead.** The graph widget's `set_evidence`/`repropagate` (`hkask-graph-widget/src/view.rs:83-99`) mutates local state and re-flows marginals in-process via `hkask_forecast::marginalize` (shared math with `hkask-mcp-scenarios::compute_marginal_probabilities`). The graph-audit diagnosed this as a "write into a void" (no effector reaches the server) and prescribed wiring dispatch. On investigation, `scenario_quantify` takes base `events` and computes marginals — there is **no MCP tool that accepts evidence overrides**, and adding one is the wrong move: (a) a *commit-evidence* tool would duplicate/undercut `scenario_update`'s Bayesian discipline (which forces the user to articulate likelihood + base rate, not assert a probability); (b) a stateless *what-if re-quantify* tool is low-value — the widget already re-flows instantly in-process, so a server round-trip adds latency for no correctness gain. The widget's job is to **model, not steer** (Ashby's requisite variety is satisfied locally: the disturbance is the user's mental model of the tree, the correction is the re-flowed marginals). The real sovereignty gap is that a *good* what-if is **throwaway** — the user explores "if X=90%, then Y" and it vanishes. The feature that fixes that is **branch/compare** (Track 3): keep the agent's tree + the user's what-if as two cached versions, side by side, and let the user record a branch (not commit a belief). That is a widget-cache feature (`VIZ_CACHE` typed handles + version key — T8a), not an MCP tool. **Decision: do not wire the graph widget to the server; extend the cache to version its local what-ifs.** The graph widget is the strongest argument for un-gating Track 3. *Ground: the evidence-override inquiry this session; `scenario_quantify`/`scenario_update` shapes in `hkask_mcp_scenarios.rs`; graph-audit's Good-Regulator verdict revisited.*

## Dependency graph (bottom-up)

```
T0  relocate ToolInvoker → hkask-tool-invoker leaf   (depends: none — foundation)
       │
       ├─> T1  delete dead EventEmitter<TransportEvent> for MediaWidget  (depends: none — parallel hygiene)
       │
       ├─> T2  wire scenarios Next:/PIPELINE_STAGES rungs to shared_tool_invoker()  (depends: T0)
       │      [CHECKPOINT C1]
       │
       ├─> T3  add BlockProvenance + scenarios field + hkask-mcp-scenarios emits it  (depends: T0)
       │       │
       │       └─> T4  scenarios provenance-driven dispatch (modified args from provenance.args)  (depends: T2, T3)
       │              [CHECKPOINT C2]
       │
       ├─> T5  portfolio fan-out: provenance + date-scrub → portfolio_returns (fixes SF-4)  (depends: T3)
       │
       └─> T6  kanban fan-out: provenance + card affordance → kanban_task_move  (depends: T3)
              [CHECKPOINT C3]
                                 ┌──────────────────────────────────────┐
                                 │  T7  measurement gate: reg.widget.reask span  (depends: T4; parallel to T5/T6)
                                 │   │
                                 │   └─> T8a  cache typed-handle + version key  (depends: T7 gate >15%)
                                 │          T8b  branch/revert UI  (depends: T8a)
                                 │          T8c  compare side-by-side UI  (depends: T8a)
                                 │          [CHECKPOINT C4]
                                 └──────────────────────────────────────┘
   (deferred Phase 4: C "I disagree" gesture, F inline drill-down — downstream of T4, gated on T7)
```

## Phased task list with checkpoints

### Phase 0 — Foundation & hygiene

**T0 — Relocate `ToolInvoker` to leaf crate `hkask-tool-invoker`** (scope: M)
- New crate `crates/hkask-tool-invoker` with `src/hkask_tool_invoker.rs` (per repo `.rules`: no `mod.rs`, descriptive lib path). Move the trait + `static TOOL_INVOKER` + `set_tool_invoker`/`shared_tool_invoker` verbatim from `crates/swarm_panel/src/tool_invoker.rs`.
- `crates/swarm_panel` depends on the leaf and re-exports (`pub use hkask_tool_invoker::{ToolInvoker, set_tool_invoker, shared_tool_invoker};`) so its call sites compile unchanged.
- `crates/zed/src/main.rs`: `PanelToolInvoker` impls `hkask_tool_invoker::ToolInvoker` (import path change only); `set_tool_invoker` call site (`main.rs:1743`) updated.
- Workspace `Cargo.toml` `[workspace.dependencies]` gains `hkask-tool-invoker`.
- **Acceptance criteria:** `./script/clippy` clean across the workspace; `swarm_panel` `fetch_all`/`clone_to_local`/`fire_agent` still compile; one existing swarm-panel dispatch test (or a smoke check) still passes; the trait body is byte-identical to the original (no behavior change).
- **Verification:** `cargo check -p hkask-tool-invoker -p swarm_panel -p zed`; `./script/clippy`; run any `swarm_panel` test that exercises `shared_tool_invoker`.
- **Dependencies:** None (foundation).
- **Files likely touched:** new `crates/hkask-tool-invoker/{Cargo.toml,src/hkask_tool_invoker.rs}`; `crates/swarm_panel/src/tool_invoker.rs` (becomes a re-export shim or is deleted with re-exports moved); `crates/swarm_panel/Cargo.toml`; `crates/zed/Cargo.toml`; `crates/zed/src/main.rs` (import + `set_tool_invoker` call); root `Cargo.toml` (workspace dep).

**T1 — Delete dead `EventEmitter<TransportEvent> for MediaWidget`** (scope: XS)
- Remove the `impl EventEmitter<TransportEvent> for MediaWidget {}` line (`crates/hkask-media-widget/src/media_widget.rs:404`). Grep-confirmed: no `cx.emit(TransportEvent::…)` in the file, no `cx.subscribe` to a `MediaWidget` entity anywhere in the crate.
- **Acceptance criteria:** `cargo check -p hkask-media-widget` passes; grep confirms no remaining `EventEmitter<TransportEvent> for MediaWidget`.
- **Verification:** `cargo check -p hkask-media-widget`; `./script/clippy`.
- **Dependencies:** None (parallel hygiene).

**CHECKPOINT C1** — After T0 + T1: workspace builds, clippy clean, existing widget + swarm-panel tests pass. Human reviews the leaf-crate relocation before any widget depends on it.

### Phase 1 — Track 1: close the dispatch gap (Slice E)

**T2 — Wire scenarios `Next:`/`PIPELINE_STAGES` rungs to dispatch** (scope: S)
- In `crates/hkask-scenarios-widget/src/view.rs`, make the `Next: {stage} → /{tool_hint}` label (`:50-59`) and the `PIPELINE_STAGES` ladder (`:532-542`) clickable. The `on_click` handler reads `shared_tool_invoker()` and calls `invoke_tool("hkask-mcp-scenarios", <tool_hint>, <hardcoded args>)` (args are the tool's defaults — provenance arrives in T4). Surface a pending/error state on the widget (`self.dispatch_in_flight: Option<String>`, `self.dispatch_error: Option<String>`); on success, `cx.notify()`.
- Add a `MockToolInvoker` in the widget's test module (implements `hkask_tool_invoker::ToolInvoker`, records `(server, tool, args)` into a `Mutex<Vec<…>>`, returns canned JSON).
- **Acceptance criteria:** clicking a rung calls `shared_tool_invoker()`; a test asserts `MockToolInvoker` received `("hkask-mcp-scenarios", "scenario_frame", <args>)`; the widget shows pending then resolves; a missing invoker (`shared_tool_invoker()` returns `None`) is surfaced as a visible error, not a silent no-op (repo `.rules` startup-failure-signal trap).
- **Verification:** `cargo test -p hkask-scenarios-widget`; `./script/clippy`; manual: render a ```` ```scenarios ```` block, click `Next: Frame`.
- **Dependencies:** T0.
- **Files likely touched:** `crates/hkask-scenarios-widget/src/view.rs`; `crates/hkask-scenarios-widget/Cargo.toml` (add `hkask-tool-invoker`, `serde_json`); `crates/hkask-scenarios-widget/src/hkask_scenarios_widget.rs` (if re-exports needed).

### Phase 2 — Track 2: provenance + fan-out

**T3 — Add `BlockProvenance` + scenarios field + MCP server emits it** (scope: M)
- Add `BlockProvenance { #[serde(default)] tool: Option<String>, server: Option<String>, args: serde_json::Value, span_id: Option<String> }` to `crates/hkask-tool-invoker/src/hkask_tool_invoker.rs` (or a `provenance` submodule).
- Add `#[serde(default)] pub provenance: BlockProvenance` to `ScenariosBlockBody` (`crates/hkask-scenarios-widget/src/block.rs`).
- `hkask-mcp-scenarios` (`kask/mcp-servers/hkask-mcp-scenarios/src/`): the `scenario_status` tool result / `display_hint` carries `provenance` so the agent can emit it in the fenced block. (Decide whether the server emits a ready-made ```` ```scenarios ```` block with provenance baked in, or whether the agent is instructed to copy it — favor the server baking it in, since the agent's copy step is where provenance could be lost/fabricated; note this as an open question.)
- **Acceptance criteria:** `ScenariosBlockBody` parses a body with `provenance` and one without (defaults empty); the existing `scaffolding_*` tests still pass; `hkask-mcp-scenarios` emits a block body containing a non-empty `provenance` for `scenario_status`.
- **Verification:** `cargo test -p hkask-scenarios-widget -p hkask-mcp-scenarios`; `./script/clippy`.
- **Dependencies:** T0.
- **Files likely touched:** `crates/hkask-tool-invoker/src/hkask_tool_invoker.rs`; `crates/hkask-scenarios-widget/src/block.rs`; `crates/hkask-scenarios-widget/Cargo.toml`; `kask/mcp-servers/hkask-mcp-scenarios/src/{hkask_mcp_scenarios.rs,templates.rs,types.rs}`.

**T4 — Scenarios provenance-driven dispatch** (scope: S)
- Extend the T2 `on_click` handlers: when `self.body.provenance.tool` is `Some` and matches the rung's tool, construct modified args by merging the rung's parameter change into `self.body.provenance.args` (e.g. a probability slider overrides the relevant field), and dispatch with `self.body.provenance.server`. Fall back to T2's hardcoded args when provenance is absent.
- **Acceptance criteria:** a test with a body carrying `provenance.tool = "scenario_quantify"` and `provenance.args = {…}` dispatches with the *merged* args (not the defaults); a body without provenance falls back to defaults; fabricated/missing provenance is surfaced, not silently dropped.
- **Verification:** `cargo test -p hkask-scenarios-widget`; `./script/clippy`.
- **Dependencies:** T2, T3.

**CHECKPOINT C2** — After T4: end-to-end scenarios dispatch works (click rung → governed MCP call → result). Human reviews the provenance contract before fanning out.

**T5 — Portfolio fan-out: provenance + date-scrub → `portfolio_returns`** (scope: M)
- Add `provenance` to `PortfolioBlockBody` (`crates/hkask-portfolio-widget/src/block.rs`); `hkask-mcp-companies` emits it.
- Add a date-range scrub affordance on the "Returns: {from} to {to}" row (`view.rs:97-120`) that dispatches `portfolio_returns` with modified `from`/`to` from `provenance.args`.
- **Fix SF-4 along the way:** validate `from`/`to` before dispatch (`kask/mcp-servers/hkask-mcp-companies/src/tools/portfolio.rs:331-361`) — return `McpToolError::invalid_argument` on parse failure instead of `unwrap_or_default()` to 1970-01-01.
- **Acceptance criteria:** date scrub dispatches `portfolio_returns` with the new range; a malformed date surfaces `invalid_argument` (not a 1970 epoch result); provenance-absent falls back to read-only display.
- **Verification:** `cargo test -p hkask-portfolio-widget -p hkask-mcp-companies`; `./script/clippy`.
- **Dependencies:** T3.
- **Files likely touched:** `crates/hkask-portfolio-widget/src/{block.rs,view.rs,hkask_portfolio_widget.rs,Cargo.toml}`; `kask/mcp-servers/hkask-mcp-companies/src/{hkask_mcp_companies.rs,tools/portfolio.rs}`.

**T6 — Kanban fan-out: provenance + card affordance → `kanban_task_move`** (scope: M)
- Add `provenance` to `KanbanBlockBody` (`crates/hkask-kanban-widget/src/block.rs`); `hkask-mcp-kata-kanban` emits it.
- Add a card affordance (e.g. a "Move" menu or drag intent) that dispatches `kanban_task_move` with `{task_id, status}` from the card + `provenance.server`. The widget stays otherwise read-only per its spec (`view.rs:5-7`); only the explicit affordance mutates.
- **Acceptance criteria:** the affordance dispatches `kanban_task_move`; a missing invoker surfaces an error; provenance-absent disables the affordance with a visible "ask the agent" hint.
- **Verification:** `cargo test -p hkask-kanban-widget -p hkask-mcp-kata-kanban`; `./script/clippy`.
- **Dependencies:** T3.

**CHECKPOINT C3** — After T5 + T6: three widgets dispatch through the governed path. Human reviews the fan-out pattern before Track 3.

### Phase 3 — Track 3: branch/compare (parallel, measurement-gated) — *the graph widget's what-if-branching track*

Track 3's primary use case is the graph widget (`hkask-graph-widget`): its `set_evidence`/`repropagate` is a rich local effector, but a good what-if ("if node X is 90%, then Y re-flows") is throwaway today. Branch/compare versions it — keep the agent's tree + the user's what-if as cached branches, side by side, and let the user record a branch (not commit a belief to the server; see decision 10). The cache rewrite is the load-bearing work; the graph widget is the evidence that Track 3 should be un-gated once the measurement justifies it.

**T7 — Measurement gate: `reg.widget.reask` + `reg.widget.whatif_discarded` spans** (scope: S)
- Instrument two signals keyed by `provenance.span_id`:
  - `reg.widget.reask` — a user sends a chat message within N turns/seconds of a widget render whose `provenance.tool` matches the message's intent (heuristic; precise matching is an open question). Aggregate the re-ask rate per widget-bearing turn.
  - `reg.widget.whatif_discarded` — a user set local evidence on a graph widget node (`set_evidence`) and then navigated away / the next agent turn overwrote the block without the user saving a branch. This is the graph-widget-specific signal that a what-if was lost; it directly argues for un-gating Track 3.
- **Gate:** >15% re-ask **or** any non-trivial `whatif_discarded` count (e.g. >5% of graph-widget renders with evidence set) → proceed to T8a; <5% on both → defer Track 3 and the Phase-4 C/F fan-out (Tracks 1–2 may suffice); in between → human decides.
- **Acceptance criteria:** both spans are emitted and queryable; the aggregate rates are computable; the gate decision is documented.
- **Verification:** run a session, confirm spans appear in the regulation store.
- **Dependencies:** T4 (needs provenance to key on); the `whatif_discarded` signal additionally needs the graph widget to record that evidence was set (a small instrumentation hook on `set_evidence`). Parallel to T5/T6.

**T8a — Cache typed-handle + version key** (scope: M) — *only if T7 gate passes*
- Change `CachedWidget` (`hkask_viz_core.rs:176-194`) to retain a typed dispatch handle (or a `Box<dyn Fn() -> AnyElement>` render closure **plus** a version/branch discriminator), and change `cache_key` (`:275-279`) from a bare `u64` body hash to a key that includes a version/branch id and verifies body equality on hit (fixes the collision risk N-3). This is what lets a graph widget's evidence-overridden tree coexist in the cache alongside the agent's original tree as a distinct branch.
- **Acceptance criteria:** two branches of the same artifact coexist in the cache; a hash collision no longer returns the wrong widget; existing `cache_key_is_stable` and `viz_factories_cover_four_widgets` tests updated and pass.
- **Verification:** `cargo test -p hkask-viz-core`; `./script/clippy`.
- **Dependencies:** T7 gate.

**T8b — Branch/revert UI** (scope: S) — *only if T8a*
- A "Keep this version / try a different assumption" affordance on a rendered widget that creates a branch (new cache entry) and a "Revert" that restores the agent's version. For the graph widget this is the "save this what-if" gesture: the user's evidence overrides become a named branch rather than a throwaway local mutation.
- **Acceptance criteria:** branching creates a distinct cached entity; revert restores the prior body's widget; a graph-widget what-if can be saved as a branch.
- **Verification:** `cargo test` on the host widget; manual.
- **Dependencies:** T8a.

**T8c — Compare side-by-side UI** (scope: S) — *only if T8a*
- Render two cached widgets (agent version + user branch) side by side for comparison. For the graph widget this is the payoff view: the base tree next to the what-if tree, marginals differing visibly.
- **Acceptance criteria:** two branches render simultaneously from the cache.
- **Verification:** manual.
- **Dependencies:** T8a.

**CHECKPOINT C4** — After T8a–T8c: branch/compare/revert works end-to-end.

### Phase 4 — Deferred downstream (gated on T7)

- **C ("I disagree" gesture)** and **F (inline drill-down)** are downstream of T4 and gated on the T7 re-ask measurement. They are not scheduled here; they become tasks when the gate passes and provenance is on ≥2 widgets.
- **D (ghost edits)** and **H (consent-gated side effects)** are policy/UI on top of the dispatch path; not architectural tasks. **I (ontology-bounded affordances)** is configuration of which buttons the dispatch exposes (FIBO tags already on portfolio tiles, `block.rs:14-24`); becomes a real seam only when a second ontology domain (PKO for scenarios, GOLEM for graph) materializes — do not pre-build a catalog trait.

## Risks

| Risk | Impact | Mitigation |
|---|---|---|
| **Provenance fabrication by the agent.** The agent emits the block, so provenance is only as honest as the agent; a user clicking "re-issue" trusts self-reported provenance. | Medium — wrong tool/args dispatched. | T3 favors the MCP server baking provenance into the `display_hint` block (server is authoritative, not the agent's copy). Add a test that the server's emitted block carries provenance; surface "unverified provenance" if the agent's block lacks it. |
| **Stale agent context after a widget commit.** Widget mutates MCP state mid-agent-turn; the agent's next read may be stale (H4). | Medium — coherence, not budget (call-caps are per-WebID). | Already tolerated in production (swarm panel). For high-lossy mutations, offer compose-back-to-agent as the dispatch mode (Track 2 leaves this as a per-action choice, not a prerequisite). Document in the widget's affordance tooltip. |
| **Tool removed/renamed server-side.** A widget's `provenance.tool` no longer exists → `NotFound`/`CapabilityDenied`. | Low-Medium. | T4/T5/T6 surface the error as "tool unavailable — ask the agent" rather than silently failing (repo `.rules` startup-failure-signal trap generalized). |
| **Cache collision (N-3) silently returns wrong widget.** Bare `u64` body hash, no equality check. | Low probability, unbounded blast radius. | Fixed in T8a for Track 3. For Track 1/2, provenance is part of the body so a re-issued result with different args → different body → different key (A does not need the cache change; only G does). |
| **Leaf-crate relocation breaks a subtle swarm-panel invariant.** | Medium — blocks all tracks. | T0 keeps the trait byte-identical; re-exports preserve call sites; C1 checkpoint gates the rest. |
| **`VizWidget` trait drift if a future effector trait is forced early.** | Low — speculative generality. | Decision 6: no effector trait in Track 1/2; extract only on second consumer. |
| **Re-ask rate is never measured, so Track 3 never gates.** | Low — Track 3 stays deferred, which is acceptable. | T7 is small; if instrumentation is too costly, the human can decide to skip Track 3 outright (sovereignty via Tracks 1–2 is already substantial). |

## Open questions

1. **Does the MCP server bake provenance into the `display_hint` block, or does the agent copy it?** Favor the server baking it in (authoritative). T3 must decide; the choice is load-bearing for the fabrication risk.
2. **`BlockProvenance` placement:** `hkask-tool-invoker` leaf (cohesive with dispatch) vs. a separate `hkask-viz-block` leaf. Decide in T3 review; split is cheap later.
3. **Re-ask detection heuristic (T7):** how to match a chat message to a prior widget's `provenance.tool` intent. Likely fuzzy; may need the agent's tool-call log. Open until T7.
4. **Compose-back vs. bypass per action class:** is there a rule (read/refetch → bypass; commit/mutate-stored-state → compose-back), or is it per-widget? The H4 grep shows the system already tolerates bypass for mutating lifecycle calls; a rule may be unnecessary. Decide at C3.
5. **Media playback perf (SF-1/2/3):** SF-2 (perpetual 30fps re-render) and SF-3 (loop fires on load-failure) are **fixed** this session — `tick_playback` gates `notify`/`set_state` on transport-state change and stops the loop when there is no loaded player. SF-1 (foreground `std::fs::read` + rodio decode) **remains open**; the safe fix offloads only the file read to `cx.background_spawn`, keeping `play_bytes` on the foreground thread. Schedule as separate work. SF-4 was fixed incidentally by T5.
6. **~~Should Track 1/2 wire the graph widget's `set_evidence` to dispatch `scenario_quantify`?~~ RESOLVED — no.** The graph widget's local effector is correctly local (instant what-if exploration is its purpose); there is no MCP tool that accepts evidence overrides, and adding one is wrong (a commit tool would undercut `scenario_update`'s Bayesian discipline; a stateless re-quantify tool is low-value since the widget already re-flows in-process). The real gap is that what-ifs are throwaway — fixed by Track 3 (branch/compare, T8a–T8c), not by a server round-trip. See decision 10.

## Refinement history (PDCA)

- **Iteration 1 (this plan):** the producer's first decomposition was corrected by the `metacognition` Architect rotation, which flagged a missing effector trait + typed cache handle as co-equal with provenance. On re-examination against `CachedWidget` (`hkask_viz_core.rs:176-194`), the cached entity is already self-mutating through its own `cx.listener` handlers, so the effector trait and typed cache handle are **not** prerequisites for Track 1/2 — only for Track 3 (branch/compare, where external code needs typed handles). The plan was refined to (a) drop the effector trait from Track 1/2 (decision 6, avoiding the `.rules` Trait-with-one-impl trap), (b) scope the cache rewrite to Track 3 only, and (c) gate Track 3 on a measured re-ask rate rather than assuming its value. The `refactor-architecture` pass additionally falsified the original "G is policy on top of A" claim (G is independent) and the "widget-held handle" framing (global accessor is cheaper and needs no trait change) — both folded into decisions 2 and 7.
- **Iteration 2 (post-implementation + evidence-override inquiry):** T0–T6 landed (Checkpoints C1–C3 green; SF-4 fixed by T5). The media SF-2/SF-3 fixes shipped separately (decision 9 updated). The evidence-override design question ("should there be an MCP tool that accepts evidence overrides?") was investigated against `scenario_quantify`/`scenario_update` and resolved **no** (decision 10): the graph widget's local effector is correctly local, and its real sovereignty gap — throwaway what-ifs — is served by Track 3 (branch/compare), not a server round-trip. Track 3 was reframed around the graph widget's what-if-branching as its primary use case, and T7 gained a second signal `reg.widget.whatif_discarded` (a what-if the user set evidence on but never saved) alongside `reg.widget.reask`; the gate now fires on either signal. Open question 6 marked RESOLVED. This iteration is the source of decision 10 and the T7/T8 reframing.