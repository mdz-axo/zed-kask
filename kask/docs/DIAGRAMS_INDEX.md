---
title: hKask Diagram Index — Mermaid Verification Registry
audience: Maintainers
last_updated: 2026-08-03
version: v0.32.0
status: living document
domain: documentation
mds_categories: [Composition, Interface, Trust, Lifecycle, Curation]
---

# hKask Diagram Index — Mermaid Verification Registry

**Purpose:** Verifiable registry of all Mermaid diagrams in the hKask documentation corpus. Per the Mermaid-First Mandate from `DOCUMENTATION_STANDARDS.md` §4: every interaction pattern, data flow, and object model is diagrammed. Every diagram carries `DIAGRAM_ALIGNMENT` metadata where applicable.

**2026-08-03 cleanup:** This registry was purged of all "PARENT DELETED" / "removed" entries — diagrams whose parent documents were deleted in the 2026-07-24 cleanup (`explanation/regulation-and-loops.md`, `explanation/architecture-patterns.md`, `explanation/sovereignty-and-ocap.md`, `explanation/federation-and-transport.md`, `how-to/deployment-and-transport.md`) and the "host status report" diagrams. Those diagrams are recoverable via git history only and are no longer tracked here. Stale paths were corrected (`how-to/training-and-adapters.md` → `explanation/training-and-adapters.md`; the `hkask-storage` schema path → `src/core/sql/schema.sql`). Newly-added surviving diagrams (the call-cap invoke gate, the swarm consent-gate sequence, the span lifecycle, several `plans/` and `research/` diagrams) and the D18 viz widgets were added.

The Diataxis documentation set (`docs/diataxis/`) carries ~40 per-crate diagrams (one per artifact across 10 crates), tracked in [`diataxis/INDEX.md`](diataxis/INDEX.md), not duplicated here.

---

## 1. Domain & Capability Diagrams

| Diagram ID | Description | Now Inline In | Verified Against | Status |
|-----------|-------------|---------------|-----------------|--------|
| DIAG-DC-CAP | hKask Capability — `DelegationToken` class model (resource/resource_id/action triple, `is_valid_for`, no expiry/signature) | `diagrams/class-hkask-capability.md` | `crates/hkask-capability/src/lib.rs`, `crates/hkask-capability/src/token_types.rs` | ✅ SURVIVES 2026-08-03 (path confirmed) |

## 2. Interface & Composition Diagrams

| Diagram ID | Description | Now Inline In | Verified Against | Status |
|-----------|-------------|---------------|-----------------|--------|
| DIAG-IC-017 | Kata-Kanban MCP Server Architecture — KanbanServer, KanbanService, KataEngine, HMemStore, Task, Board, TaskStatus class relationships | `reference/mcp-servers/README.md` (Kata-Kanban Server Architecture section) | `mcp-servers/hkask-mcp-kata-kanban/src/lib.rs`, `mcp-servers/hkask-mcp-kata-kanban/src/kanban/service_impl/service.rs`, `mcp-servers/hkask-mcp-kata-kanban/src/kata.rs`, `crates/hkask-storage/src/hmem.rs` | ✅ VERIFIED 2026-07-20 |
| DIAG-IC-INVOKE | MCP Runtime Invoke — Simplified Gate Flow (capability-match → per-agent `CallCap` `can_proceed`+`charge_call` → dispatch → span) | `diagrams/flowchart-mcp-runtime-invoke.md` | `crates/hkask-mcp/src/runtime.rs` (`ToolPort::invoke`), `crates/hkask-regulation/src/energy.rs` (`CallCapManager`) | ✅ SURVIVES 2026-08-03 (updated for the gas→call-cap refactor) |
| DIAG-IC-BRIDGE | Composition root sequence — main.rs → kask_bridge → agent.rs hooks → kask panel | `diataxis/kask_bridge/explanation.md` | `crates/kask_bridge/`, `crates/zed/src/main.rs` | ✅ SURVIVES 2026-08-03 (diataxis) |
| DIAG-IC-TYPES | hkask-types composition-root sequence — GuardedInferencePort / LanguageModelInferencePort / OnceLock hooks / BridgeManifestExecutor | `diataxis/hkask-types/explanation.md` | `crates/hkask-types/`, `crates/kask_bridge/` | ✅ SURVIVES 2026-08-03 (diataxis) |

## 3. Trust & Observability Diagrams

| Diagram ID | Description | Now Inline In | Verified Against | Status |
|-----------|-------------|---------------|-----------------|--------|
| DIAG-TO-SPAN | Regulation Span Lifecycle — Emission → Storage → Query → Decay (WeightedEvent EMA) | `reference/regulation-spans.md` §4 | `crates/hkask-regulation/src/runtime.rs`, `crates/hkask-storage/src/regulation_store.rs`, `crates/hkask-types/src/event.rs` | ✅ SURVIVES 2026-08-03 (path confirmed) |
| DIAG-TO-NUEVENT | Nu-Event Semantics — emitter → RegulationRecord → RegulationSink → RegulationArchive → sensors/CyberneticsLoop/CurationLoop | `explanation/cognition-and-replica.md` §2 | `crates/hkask-regulation/src/runtime.rs`, `crates/hkask-types/src/event.rs` | ✅ SURVIVES 2026-08-03 (path confirmed) |

## 4. Persistence & Lifecycle Diagrams

| Diagram ID | Description | Now Inline In | Verified Against | Status |
|-----------|-------------|---------------|-----------------|--------|
| DIAG-PL-003 | Memory Architecture — Episodic/Semantic public/private gating, consolidation bridge | `explanation/cognition-and-replica.md` (Memory Pipeline section) | `crates/hkask-memory/src/` | ✅ VERIFIED 2026-07-01 |
| DIAG-PL-STORAGE | hkask-storage ERD — core tables (hmems, embeddings, nu_events, audit_log, kata_history, pod_meta) | `diataxis/hkask-storage/reference.md` | `crates/hkask-storage/src/core/sql/schema.sql` | ✅ SURVIVES 2026-08-03 (wallet/goals tables removed 2026-08-03) |
| DIAG-PL-HMEM | Bitemporal hMem state machine (Active → Superseded/Recalled) | `diataxis/hkask-storage/explanation.md` | `crates/hkask-storage/src/hmem.rs` | ✅ SURVIVES 2026-08-03 (diataxis) |
| DIAG-PL-GALLERY | Media Gallery Schema ERD — Gallery/Image/Tag/Metadata/Policy/DerivedWork | `research/media-research/design-schema.md` (T4) | `crates/hkask-storage/src/gallery.rs` | ✅ SURVIVES 2026-08-03 (path confirmed) |
| DIAG-PL-MEDIALAND | Media Tool Domain Landscape — ImageTool/VideoTool → Model → ProviderEndpoint, InputType/OutputType | `research/media-research/media-landscape.md` (T1) | `mcp-servers/hkask-mcp-media/` | ✅ SURVIVES 2026-08-03 (path confirmed) |

## 5. Framework & Methodology Diagrams

| Diagram ID | Description | Now Inline In | Verified Against | Status |
|-----------|-------------|---------------|-----------------|--------|
| DIAG-FW-001 | MDS RDF/Turtle Semantic Graph | `architecture/core/MDS.md` §1.1 | `docs/architecture/core/MDS.md` (textual RDF reference) | ✅ VERIFIED 2026-07-01 |
| DIAG-FW-002 | MDS Entity Relationship Diagram (Spec ↔ Goal ↔ Curation) | `architecture/core/MDS.md` §1.2 | `docs/architecture/core/MDS.md` (textual ERD reference) | ✅ VERIFIED 2026-07-01 |
| DIAG-FW-003 | MVSDD Cycle Sequence Diagram (Specify → Grant → Compose → Curate → Reflect) | `architecture/core/MDS.md` §4.3 | `docs/architecture/core/MDS.md` (textual cycle reference) | ✅ VERIFIED 2026-07-01 |
| DIAG-FW-DEPS | Composition-root dependency direction — zed-kask surfaces → kask_bridge → MCP servers → hKask domain crates | `architecture/core/MDS.md` (Dependency Direction section) | `docs/architecture/core/MDS.md` | ✅ SURVIVES 2026-08-03 (path confirmed) |
| DIAG-FW-LIFECYCLE | Documentation lifecycle state machine (Draft → Active → Deprecated/Superseded → Removed) | `architecture/DOCUMENTATION_STANDARDS.md` §3 | `docs/architecture/DOCUMENTATION_STANDARDS.md` | ✅ SURVIVES 2026-08-03 (path confirmed) |

## 6. Reference Diagrams

| Diagram ID | Description | Now Inline In | Verified Against | Status |
|-----------|-------------|---------------|-----------------|--------|
| DIAG-RF-004 | Companies tool routing and dispatch flow — combined_router (7 sub-routers) → execute_tool seam → three sinks | `reference/mcp-servers/companies.md` | `mcp-servers/hkask-mcp-companies/src/lib.rs`, `mcp-servers/hkask-mcp-companies/src/tools/mod.rs`, `mcp-servers/hkask-mcp-companies/src/providers.rs`, `mcp-servers/hkask-mcp-companies/src/portfolio.rs` | ✅ VERIFIED 2026-07-17 |
| DIAG-RF-005 | Scenario Forecasting Pipeline — 18 MCP tools grouped by pipeline phase | `reference/mcp-servers/scenarios.md` | `mcp-servers/hkask-mcp-scenarios/src/lib.rs`, `mcp-servers/hkask-mcp-scenarios/src/superforecast.rs`, `mcp-servers/hkask-mcp-scenarios/src/types.rs` | ✅ VERIFIED 2026-07-21 |
| DIAG-RF-006 | Condenser MCP Server pipeline — CondenserServer tool router + compression pipeline | `reference/mcp-servers/condenser.md` | `mcp-servers/hkask-mcp-condenser/` | ✅ SURVIVES 2026-08-03 (path confirmed) |
| DIAG-RF-SWARM | Swarm consent gate sequence — panel → swarm_hire_cost → ABW → swarm_request_consent → consume (single-use, TTL) | `reference/mcp-servers/swarm.md` | `mcp-servers/hkask-mcp-swarm/src/consent.rs`, `mcp-servers/hkask-mcp-swarm/src/spend_gate.rs` | ✅ SURVIVES 2026-08-03 (path confirmed) |

## 9. Training and Corpus Diagrams

| Diagram ID | Type | Description | Now Inline In | Verified Against | Status |
|------------|------|-------------|---------------|------------------|--------|
| DIAG-TRAIN-001 | flowchart | Unsloth Qwen3.6-27B training pipeline | `explanation/training-and-adapters.md` | HF: `Axolotl-Partners/rust-adapter-scripts` | ✅ VERIFIED 2026-07-10 (path corrected from `how-to/`) |
| DIAG-TRAIN-002 | flowchart | Corpus, replica, and training readiness boundary | `explanation/training-and-adapters.md` | `hkask-mcp-corpus`, `hkask-mcp-training` | ✅ VERIFIED 2026-07-10 (path corrected) |
| DIAG-TRAIN-003 | flowchart | Corpus pipeline dispatch and unsupported-step boundary | `explanation/training-and-adapters.md` | `hkask-mcp-corpus`, `hkask-types`, `hkask-mcp` | ✅ VERIFIED 2026-07-10 (path corrected) |
| DIAG-TRAIN-004 | flowchart | Full training pipeline (reasoning + Rust adapters + eval) | `explanation/training-and-adapters.md` | HF: `Axolotl-Partners/rust-adapter-scripts` | ✅ VERIFIED 2026-07-11 (path corrected) |
| DIAG-TRAIN-005 | state | Training job lifecycle: Queued → Running → Completed → Terminated | `explanation/training-and-adapters.md` | `hkask-mcp-training/src/providers/types.rs`, HF: `Axolotl-Partners/rust-adapter-scripts` | ✅ VERIFIED 2026-07-11 (path corrected) |
| DIAG-TRAIN-006 | class | Training server type hierarchy: TrainingHost, HarnessAdapter, PodStatus, params | `explanation/training-and-adapters.md` | `hkask-mcp-training/src/providers/{types,runpod,deepinfra,nebius,harness,trl_harness}.rs` | ✅ VERIFIED 2026-07-23 (path corrected) |

## 11. Additional Inlined Diagrams (surviving only)

These standalone diagram files were inlined into parent documents. Only the survivors are listed; the parent-deleted ones (architecture-patterns, regulation-and-loops, sovereignty-and-ocap, deployment-and-transport) were removed from this registry in the 2026-08-03 cleanup — recoverable via git.

| Diagram File (former) | Type | Now Inline In | Description | Status |
|----------------------|------|---------------|-------------|--------|
| `flowchart-algo-classification.md` | flowchart | `explanation/cognition-and-replica.md` | Algo / No-Judge Classification Flow | ✅ SURVIVES |
| `flowchart-memory-remember.md` | flowchart | `explanation/cognition-and-replica.md` | Memory Remember — Algo / No-Judge Template Cascade | ✅ SURVIVES |
| `sequence-classify-to-memory.md` | sequence | `explanation/cognition-and-replica.md` | Classification-to-Memory Sequence | ✅ SURVIVES |
| `flowchart-scenario-forecasting.md` | flowchart | `explanation/cognition-and-replica.md` §1 | Scenario forecasting 5-phase pipeline (Frame → Brainstorm → Quantify → Synthesize → Track → Assess) | ✅ SURVIVES 2026-08-03 |
| `flowchart-companies-mcp.md` | flowchart | `explanation/cognition-and-replica.md` §3 | Companies MCP — provider routing → valuation → forecast store → portfolio ledger | ✅ SURVIVES 2026-08-03 |

## 12. Swarm System Diagrams

The swarm system (`hkask-mcp-swarm` + `crates/swarm_panel` + the `swarm-intelligence` / `swarm-steering` skills) is documented cross-cuttingly in `diagrams/` and the `diataxis/swarm_system/` set. These standalone diagram files survive in `docs/diagrams/`.

| Diagram ID | Type | File | Description | Verified Against | Status |
|-----------|------|------|-------------|------------------|--------|
| DIAG-DIA-SWARM-001 | flowchart | `diagrams/flowchart-swarm-architecture.md` | Swarm MCP server architecture — two launch paths, two substrates, panel + skills + curator | `hkask_mcp_swarm.rs`, `swarm_panel.rs` | ✅ VERIFIED 2026-08-03 |
| DIAG-DIA-SWARM-002 | flowchart | `diagrams/flowchart-swarm-pdca-cascade.md` | swarm-intelligence 10-step PDCA cascade with deterministic compute steps | `swarm-intelligence/SKILL.md` | ✅ VERIFIED |
| DIAG-DIA-SWARM-003 | sequence | `diagrams/sequence-swarm-steering-loop.md` | Steering loop — advisory vs steering execution, delegate_results feedback | `swarm-steering/SKILL.md` | ✅ VERIFIED |
| DIAG-DIA-SWARM-006 | class | `diagrams/class-swarm-server.md` | SwarmServer collaborators — AbwClient, ConsentStore, SpendGate, runtime, executor, A2A, LocalDelegateResult | `hkask_mcp_swarm.rs`, `consent.rs`, `spend_gate.rs`, `local_runtime.rs`, `agent_executor.rs`, `a2a.rs` | ✅ VERIFIED 2026-08-03 |
| DIAG-DIA-SWARM-007 | state | `diagrams/state-swarm-panel-modes.md` | SwarmPanel PanelMode states (Browse/Author/Compose/Steer) + backend toggle | `swarm_panel.rs` | ✅ VERIFIED 2026-08-03 |
| DIAG-DIA-SWARM-008 | flowchart | `diagrams/flowchart-swarm-feedback-loops.md` | Four feedback loops with 5-property health + algedonic override + C4 latency deficit | `swarm-intelligence/SKILL.md`, `consent.rs`, `swarm_panel.rs` | ✅ VERIFIED 2026-08-03 |

**Companion audit:** [`audits/swarm-cybernetics-semantics-audit.md`](audits/swarm-cybernetics-semantics-audit.md) — pragmatic-semantics gap analysis + pragmatic-cybernetics per-property loop assessment + VSM map + Ashby variety check.

## 13. Plan & Design Diagrams (newly indexed)

These live under `docs/plans/` and `docs/research/` and were not previously tracked in this registry.

| Diagram ID | Type | Now Inline In | Description | Verified Against | Status |
|-----------|------|---------------|-------------|------------------|--------|
| DIAG-PLAN-SWARM-A | flowchart | `plans/cybernetic-swarm-plan.md` §3 | Three desiderata × hKask dependency hierarchy (D1 Reliability → D2 Lifelong → D3 Self-Improvement) | `plans/cybernetic-swarm-plan.md` | ✅ SURVIVES 2026-08-03 (path confirmed) |
| DIAG-PLAN-SWARM-B | flowchart | `plans/cybernetic-swarm-plan.md` §8 | Implementation sequencing — C0/C2/C1/C4/C5 step dependency | `plans/cybernetic-swarm-plan.md` | ✅ SURVIVES 2026-08-03 (path confirmed) |
| DIAG-PLAN-SWARM-C | flowchart | `plans/cybernetic-swarm-plan.md` §9.1 | Complete cybernetic swarm map (revision 2 — fusion removed, deterministic judge) | `plans/cybernetic-swarm-plan.md` | ✅ SURVIVES 2026-08-03 (path confirmed) |
| DIAG-PLAN-HARNESS | graph | `plans/evolving-test-harness.md` §3.1 | Evolving test harness target architecture — CI Evaluator + Proposer + trace filesystem | `plans/evolving-test-harness.md`, `crates/hkask-test-harness/` | ✅ SURVIVES 2026-08-03 (path confirmed) |
| DIAG-PLAN-SIGN-A | flowchart | `plans/kask-skill-signing-and-trust.md` (Architecture) | Skill signing & trust — package → keychain → sign → install → verify | `crates/kask_extensions_ui/`, `crates/hkask-keystore/` | ✅ SURVIVES 2026-08-03 (path confirmed) |
| DIAG-PLAN-SIGN-B | state | `plans/kask-skill-signing-and-trust.md` (Skill lifecycle) | Skill lifecycle — Local → Published → Verified → Listed → Expired → Purged | `crates/kask_extensions_ui/` | ✅ SURVIVES 2026-08-03 (path confirmed) |
| DIAG-PLAN-MEDIA | flowchart | `plans/media-system-refactor.md` §1.1 | Media two-process split — zed foreground (MediaRouter/FalBackend/DeepInfraBackend) vs MCP server | `mcp-servers/hkask-mcp-media/`, `crates/hkask-inference/` | ✅ SURVIVES 2026-08-03 (path confirmed) |
| DIAG-PLAN-WIKI | flowchart | `plans/semantic-memory-wiki.md` §5 | Semantic memory wiki architecture — sources → raw → hMem semantic → consolidation → wiki | `plans/semantic-memory-wiki.md` | ✅ SURVIVES 2026-08-03 (path confirmed) |

## 14. Viz Widgets (D18) — class diagrams

The D18 viz widgets render fenced code blocks (```media, ```graph, ```kanban, ```portfolio, ```scenarios) inline in agent markdown via `hkask-viz-core`'s composed `block_renderer`. They live under `crates/` (zed-kask-side, because they render GPUI elements and must depend on `gpui`/`theme`). Class diagrams for each were added 2026-08-03.

| Diagram ID | Widget crate | Block keyword | Renders | Diagram | Source anchor | Status |
|-----------|--------------|--------------|---------|---------|---------------|--------|
| DIAG-VIZ-CORE | `crates/hkask-viz-core` | (registry) | composes all widget renderers into one `MediaBlockRendererFn` callback + LRU entity cache | `diagrams/class-hkask-viz-core.md` | `crates/hkask-viz-core/src/hkask_viz_core.rs` | ✅ VERIFIED 2026-08-03 |
| DIAG-VIZ-MEDIA | `crates/hkask-media-widget` | `media` | image / SVG / audio / video blocks | `diagrams/class-hkask-media-widget.md` | `crates/hkask-media-widget/src/{media_ref,media_widget,audio_player,transport,video_decoder}.rs` | ✅ VERIFIED 2026-08-03 |
| DIAG-VIZ-GRAPH | `crates/hkask-graph-widget` | `graph` | event-tree DAG layout + evidence re-propagation | `diagrams/class-hkask-graph-widget.md` | `crates/hkask-graph-widget/src/{block,layout,propagate,view}.rs` | ✅ VERIFIED 2026-08-03 |
| DIAG-VIZ-KANBAN | `crates/hkask-kanban-widget` | `kanban` | kanban board columns (replaces deleted `KanbanBoardView`) | `diagrams/class-hkask-kanban-widget.md` | `crates/hkask-kanban-widget/src/{block,view}.rs` | ✅ VERIFIED 2026-08-03 |
| DIAG-VIZ-PORTFOLIO | `crates/hkask-portfolio-widget` | `portfolio` | portfolio dashboard (replaces deleted `PortfolioDashboardView`) | `diagrams/class-hkask-portfolio-widget.md` | `crates/hkask-portfolio-widget/src/{block,view}.rs` | ✅ VERIFIED 2026-08-03 |
| DIAG-VIZ-SCENARIOS | `crates/hkask-scenarios-widget` | `scenarios` | scenario pipeline / matrix / timeline (replaces deleted `ScenariosView`) | `diagrams/class-hkask-scenarios-widget.md` | `crates/hkask-scenarios-widget/src/{block,view}.rs` | ✅ VERIFIED 2026-08-03 |

Wiring seam: `crates/agent_ui/src/conversation_view.rs` — `render_agent_markdown` calls `.media_block_renderer(hkask_viz_core::block_renderer())`. See `DIVERGENCE.md` D10 and D18.

## 15. Diataxis Per-Crate Diagrams

The `docs/diataxis/` set carries one diagram per artifact across 10 crates (`hkask-capability`, `hkask-condenser`, `hkask-inference`, `hkask-mcp-server`, `hkask-regulation`, `hkask-storage`, `hkask-templates`, `hkask-types`, `kask_bridge`, `swarm_system`) — ~40 diagrams total (explanation / how-to / reference / tutorial per crate). These are tracked in [`diataxis/INDEX.md`](diataxis/INDEX.md) and are not duplicated row-by-row here. A few diataxis diagrams of cross-cutting interest are also listed above (DIAG-IC-BRIDGE, DIAG-IC-TYPES, DIAG-PL-STORAGE, DIAG-PL-HMEM).

## 16. Summary

**Surviving diagram inventory (2026-08-03):**

| Location | Count |
|----------|-------|
| `docs/diagrams/` standalone (swarm + capability + invoke-gate) | 8 |
| `docs/explanation/` (cognition-and-replica, training-and-adapters, skills-and-composition) | ~12 |
| `docs/reference/mcp-servers/` (README, companies, scenarios, condenser, swarm) | 5 |
| `docs/reference/regulation-spans.md` | 1 |
| `docs/architecture/` (MDS ×4, DOCUMENTATION_STANDARDS ×1) | 5 |
| `docs/plans/` (cybernetic-swarm ×3, evolving-test-harness, kask-skill-signing ×2, media-system-refactor, semantic-memory-wiki) | 8 |
| `docs/research/media-research/` (gallery ERD, media landscape) | 2 |
| `docs/diataxis/` (10 crates × ~4) | ~40 |
| **Total surviving** | **~81** |

**Removed from this registry (2026-08-03):** all "PARENT DELETED" / "removed — host status report" entries (~26 diagrams whose parents were deleted in the 2026-07-24 cleanup). Recoverable via git history.

**Widgets (D18):** 6 viz-widget crates registered as source-anchored candidates (no `.md` diagram yet).

**MDS completeness:** all five MDS categories retain diagram coverage.

---

## References

[^mds]: hKask Team. (2026). *MDS — Minimal Domain Specification*. `docs/architecture/core/MDS.md`.
[^doc-standards]: hKask Team. (2026). *Documentation Standards*. `docs/architecture/DOCUMENTATION_STANDARDS.md`.
[^divergence]: zed-kask Team. (2026). *Divergence surface*. `DIVERGENCE.md` — D10 (kask_panel removal), D18 (viz widgets).

---

*ℏKask v0.32.0 — A Sovereign Chat Client for Human Users with AI Skills — Diagram Verification Registry*
*Mermaid-First Mandate: Every interaction pattern, data flow, and object model is diagrammed.*
*2026-08-03: deleted-parent entries purged; new surviving diagrams + D18 widgets added; stale paths corrected.*