---
title: "Diagram Verification Registry"
audience: [developers, architects, agents]
last_updated: 2026-08-03
version: "0.32.1"
status: "Active"
domain: "Cross-cutting"
mds_categories: [lifecycle, curation]
---

# hKask Diagram Index — Mermaid Verification Registry

**Purpose:** Verifiable registry of all Mermaid diagrams in the hKask documentation corpus. Per the Mermaid-First Mandate from `DOCUMENTATION_STANDARDS.md` §4: every interaction pattern, data flow, and object model is diagrammed. Every diagram carries `DIAGRAM_ALIGNMENT` metadata.

**Consolidation status (2026-07-29):** Most standalone diagram files from the former `docs/diagrams/` directory were inlined into their parent documents per `DOCUMENTATION_STANDARDS.md` §1. A subsequent cleanup (2026-07-24) deleted several parent documents whose diagrams are now recoverable only via git history. The Diataxis documentation set (`docs/diataxis/`) added 40 new diagrams (one per artifact across 10 crates) that are tracked in `diataxis/INDEX.md`, not in this registry. This registry maps each surviving diagram to the document where it currently resides; deleted parent documents are marked.

---

## 1. Domain & Capability Diagrams

| Diagram ID | Description | Now Inline In | Verified Against | Status |
|-----------|-------------|---------------|-----------------|--------|
| DIAG-DC-004 | OCAP Capability Attenuation Chain (depth ≤ 7) | (deleted — `explanation/sovereignty-and-ocap.md` removed 2026-07-24; recoverable via git) | `crates/hkask-capability/src/lib.rs` | ⚠️ PARENT DELETED 2026-07-24 |
| DIAG-DC-005 | MCP Tool Dispatch with OCAP constraint enforcement | (deleted — `explanation/architecture-patterns.md` removed 2026-07-24; recoverable via git) | `crates/hkask-mcp/src/runtime.rs:59`, `crates/hkask-mcp/src/security.rs` | ⚠️ PARENT DELETED 2026-07-24 |
| DIAG-DC-008 | Adapter Lifecycle State Machine (Cold → Warming → Active → Draining → Removed) | `explanation/federation-and-transport.md` | `mcp-servers/hkask-mcp-training/src/adapter/endpoint_lifecycle.rs`, `mcp-servers/hkask-mcp-training/src/adapter/adapter_router/mod.rs` | ✅ VERIFIED 2026-07-01 |
| DIAG-DC-012 | Research Compound Search Flow — validate → cache → strategy → join_all → RRF fusion → rerank → deep extract → record | (removed - host status report deleted; recoverable via git) | `mcp-servers/hkask-mcp-research/src/research/providers/mod.rs:213-410,516-620`, `mcp-servers/hkask-mcp-research/src/lib.rs:265-375` | ✅ VERIFIED 2026-07-17 |
| DIAG-DC-013 | CodeGraph Architecture — CodeGraphServer, indexed_once flag, IndexPipeline, GraphStore, InferenceIpcClient, Jinja | (removed - host status report deleted; recoverable via git) | `mcp-servers/hkask-mcp-codegraph/src/lib.rs:24-31,33-76,159-548`, `mcp-servers/hkask-mcp-codegraph/src/codegraph/lib.rs:20-31`, `mcp-servers/hkask-mcp-codegraph/src/codegraph/indexer/pipeline.rs:22-273`, `mcp-servers/hkask-mcp-codegraph/src/codegraph/graph/mod.rs:1-7` | ✅ VERIFIED 2026-07-20 |

## 2. Interface & Composition Diagrams

| Diagram ID | Description | Now Inline In | Verified Against | Status |
|-----------|-------------|---------------|-----------------|--------|
| DIAG-IC-002 | Hexagonal Architecture — Ports, Adapters, Core | (deleted — `explanation/architecture-patterns.md` removed 2026-07-24; recoverable via git) | `crates/hkask-types/src/` (7 port traits) | ⚠️ PARENT DELETED 2026-07-24 |
| DIAG-IC-004 | Template Cascade Flow (depth ≤ 7, DependencyGraph acyclic) | (deleted — `explanation/architecture-patterns.md` removed 2026-07-24; recoverable via git) | `crates/hkask-templates/src/executor.rs` | ⚠️ PARENT DELETED 2026-07-24 |
| DIAG-IC-005 | Rendering Pipeline — Template → Jinja2 → LLM | (deleted — `explanation/architecture-patterns.md` removed 2026-07-24; recoverable via git) | `crates/hkask-templates/src/` (minijinja integration) | ⚠️ PARENT DELETED 2026-07-24 |
| DIAG-IC-007 | MCP Tool Dispatch Sequence with OCAP Enforcement | (deleted — `explanation/architecture-patterns.md` removed 2026-07-24; recoverable via git) | `crates/hkask-mcp/src/runtime.rs`, `crates/hkask-mcp/src/security.rs` | ⚠️ PARENT DELETED 2026-07-24 |

| DIAG-IC-012 | Regulation Architecture — responsibility clusters, wallet port, extraction status | (deleted — `explanation/regulation-and-loops.md` removed 2026-07-24; recoverable via git) | `crates/hkask-regulation/src/cybernetics_loop.rs`, `crates/hkask-regulation/src/runtime.rs`, `crates/hkask-regulation/src/wallet_budget.rs`, `crates/hkask-regulation/src/slo_manager.rs`, `crates/hkask-regulation/src/seam_watcher.rs`, `crates/hkask-types/src/wallet_budget_port.rs` | ⚠️ PARENT DELETED 2026-07-24 |
| DIAG-IC-013 | Research MCP Server Architecture — ResearchServer, ProviderPool, WebSearchPort, cache, rate limiter, RSS DB | (removed - host status report deleted; recoverable via git) | `mcp-servers/hkask-mcp-research/src/lib.rs:41-48`, `mcp-servers/hkask-mcp-research/src/research/providers/mod.rs:130-135,494-620` | ✅ VERIFIED 2026-07-17 |
| DIAG-IC-014 | Research Provider Trait Hierarchy — WebSearchPort, WebSearchProvider, WebExtractProvider, WebBrowseProvider, 9 concrete providers | (removed - host status report deleted; recoverable via git) | `mcp-servers/hkask-mcp-research/src/research/providers/mod.rs:50-135`, `mcp-servers/hkask-mcp-research/src/research/providers/brave.rs:18`, `mcp-servers/hkask-mcp-research/src/research/providers/firecrawl.rs:28,100,181` | ✅ VERIFIED 2026-07-17 |

| DIAG-IC-016 | CodeGraph Tool Dispatch Flow — execute_tool → ensure_indexed (indexed_once check) → lock pipeline → graph operation → JSON response | (removed - host status report deleted; recoverable via git) | `mcp-servers/hkask-mcp-codegraph/src/lib.rs:34-76,163-181,431-455`, `mcp-servers/hkask-mcp-codegraph/src/codegraph/indexer/pipeline.rs:61-159,245-263` | ✅ VERIFIED 2026-07-20 |
| DIAG-IC-017 | Kata-Kanban MCP Server Architecture — KanbanServer, KanbanService, KataEngine, HMemStore, Task, Board, TaskStatus, SocraticRole class relationships | `reference/mcp-servers/README.md` (Kata-Kanban Server Architecture section) | `mcp-servers/hkask-mcp-kata-kanban/src/lib.rs:29-33`, `mcp-servers/hkask-mcp-kata-kanban/src/kanban/service_impl/service.rs:34-37`, `mcp-servers/hkask-mcp-kata-kanban/src/kata.rs:76-94`, `crates/hkask-storage/src/hmem.rs:134-138` | ✅ VERIFIED 2026-07-20 (inlined from `diagrams/class-kata-kanban-architecture.md`) |

## 3. Trust & Observability Diagrams

| Diagram ID | Description | Now Inline In | Verified Against | Status |
|-----------|-------------|---------------|-----------------|--------|
| DIAG-TO-002 | OCAP Boundary Enforcement Flow | (deleted — `explanation/sovereignty-and-ocap.md` removed 2026-07-24; recoverable via git) | `crates/hkask-mcp/src/security.rs` (SecurityGateway) | ⚠️ PARENT DELETED 2026-07-24 |
| DIAG-TO-004 | Regulation Span Emission Flow (4 namespaces → Sink) | (deleted — `explanation/regulation-and-loops.md` removed 2026-07-24; recoverable via git) | `crates/hkask-regulation/src/runtime.rs`, `crates/hkask-types/src/event.rs` | ⚠️ PARENT DELETED 2026-07-24 |
| DIAG-TO-005 | Algedonic Alert Escalation (variety deficit > threshold → Curator/Human) | (deleted — `explanation/regulation-and-loops.md` removed 2026-07-24; recoverable via git) | `crates/hkask-regulation/src/algedonic.rs` | ⚠️ PARENT DELETED 2026-07-24 |
| DIAG-TO-006 | Regulation Span Emission and Algedonic Alert End-to-End Flow | (deleted — `explanation/regulation-and-loops.md` removed 2026-07-24; recoverable via git) | `crates/hkask-regulation/src/cybernetics_loop.rs`, `crates/hkask-regulation/src/algedonic.rs` (curator agent module in zed-kask; `hkask-pods` deleted 2026-07-25) | ⚠️ PARENT DELETED 2026-07-24 |
| DIAG-TO-006-CM | ConsentManager Authorization Flow | (deleted — `explanation/sovereignty-and-ocap.md` removed 2026-07-24; recoverable via git) | `hkask-types::visibility` (replaces deleted `crates/hkask-pods/src/consent.rs` and `sovereignty.rs`), `crates/hkask-storage/src/consent_store.rs` | ⚠️ PARENT DELETED 2026-07-24 |

## 4. Persistence & Lifecycle Diagrams

| Diagram ID | Description | Now Inline In | Verified Against | Status |
|-----------|-------------|---------------|-----------------|--------|
| DIAG-PL-003 | Memory Architecture — Episodic/Semantic public/private gating | `explanation/cognition-and-replica.md` | `crates/hkask-memory/src/` | ✅ VERIFIED 2026-07-01 |

## 5. Framework & Methodology Diagrams

| Diagram ID | Description | Now Inline In | Verified Against | Status |
|-----------|-------------|---------------|-----------------|--------|
| DIAG-FW-001 | MDS RDF/Turtle Semantic Graph | `architecture/core/MDS.md` §1.1 | `docs/architecture/core/MDS.md` (textual RDF reference) | ✅ VERIFIED 2026-07-01 |
| DIAG-FW-002 | MDS Entity Relationship Diagram (Spec ↔ Goal ↔ Curation) | `architecture/core/MDS.md` §1.2 | `docs/architecture/core/MDS.md` (textual ERD reference) | ✅ VERIFIED 2026-07-01 |
| DIAG-FW-003 | MVSDD Cycle Sequence Diagram (Specify → Grant → Compose → Curate → Reflect) | `architecture/core/MDS.md` §4.3 | `docs/architecture/core/MDS.md` (textual cycle reference) | ✅ VERIFIED 2026-07-01 |
| DIAG-FW-004 | Hexagonal Component Diagram (HKaskHexagon) | (deleted — `explanation/architecture-patterns.md` removed 2026-07-24; recoverable via git) | `crates/hkask-types/src/` | ⚠️ PARENT DELETED 2026-07-24 |


## 6. Reference Diagrams

| Diagram ID | Description | Now Inline In | Verified Against | Status |
|-----------|-------------|---------------|-----------------|--------|

| DIAG-DOC-001 | hKask Documentation Structure — Diataxis navigation map (quadrants + supporting directories) | (deleted — `README.md` no longer contains a Mermaid diagram as of 2026-07-29; recoverable via git) | `docs/README.md`, `docs/specifications/DOCUMENTATION_STANDARDS.md`, `docs/` directory listing | ⚠️ PARENT NO LONGER CONTAINS DIAGRAM 2026-07-29 |
| DIAG-RF-004 | Companies tool routing and dispatch flow — combined_router (7 sub-routers) → execute_tool seam → three sinks (provider fetch, valuation engines → StoredForecast, PortfolioManager spawn_blocking) | `reference/mcp-servers/companies.md` | `mcp-servers/hkask-mcp-companies/src/lib.rs:499-509,368-495`, `mcp-servers/hkask-mcp-companies/src/tools/mod.rs:1-8`, `mcp-servers/hkask-mcp-companies/src/providers.rs:111-198`, `mcp-servers/hkask-mcp-companies/src/portfolio.rs:290-340` | ✅ VERIFIED 2026-07-17 (standalone duplicate deleted) |
| DIAG-RF-005 | Scenario Forecasting Pipeline — 18 MCP tools grouped by pipeline phase (Framing → Ideation → Structuring → Computation → Aggregation → Tracking → Assessment) with engine delegation | `reference/mcp-servers/scenarios.md` | `mcp-servers/hkask-mcp-scenarios/src/lib.rs`, `mcp-servers/hkask-mcp-scenarios/src/superforecast.rs`, `mcp-servers/hkask-mcp-scenarios/src/types.rs` | ✅ VERIFIED 2026-07-21 (inlined from `diagrams/flowchart-scenario-forecasting-pipeline.md`) |


## 7. Undocumented Interaction Patterns (V1.1+ Candidates)

These interaction patterns exist in the codebase but lack dedicated diagram coverage. They are candidates for v1.1+ diagram work.

| Pattern | MDS Category | Crates Involved | Priority |
|---------|----------------|----------------|----------|
| Federation Message Flow (deferred) | Composition | `hkask-*` (deferred to v1.1+) | P2 |
| Competition Socket Protocol (ACP) | Interface | `hkask-pods` (ACP) — `hkask-pods` deleted 2026-07-25; ACP deferred | P2 |
| Git CAS Content-Addressed Blob Flow | Persistence | `hkask-storage (git_cas)` — `hkask-git-cas` deleted 2026-07-25; `GitCASPort` deleted from `hkask-types` | P2 |
| Template Manifest Validation Flow (ContractValidator) | Composition | `hkask-templates` | P2 |
| MVSDD Cycle (Specify → Grant → Compose → Curate → Reflect) | Curation | `hkask-templates`, zed-kask curator agent (replaces deleted `hkask-pods`) | P2 |

> **Note (2026-06-09):** `hkask-mcp-memory` consolidates episodic and semantic memory operations. Its interaction patterns with the memory subsystem are now covered by DIAG-PL-003 (inlined in `explanation/cognition-and-replica.md`).

---

## 9. Training and Corpus Diagrams

| Diagram ID | Type | Description | Now Inline In | Verified Against | Status |
|------------|------|-------------|---------------|------------------|--------|
| DIAG-TRAIN-001 | flowchart | Unsloth Qwen3.6-27B training pipeline | `how-to/training-and-adapters.md` | HF: `Axolotl-Partners/rust-adapter-scripts` | ✅ VERIFIED 2026-07-10 |
| DIAG-TRAIN-002 | flowchart | Corpus, replica, and training readiness boundary | `how-to/training-and-adapters.md` | `hkask-mcp-corpus`, `hkask-mcp-training` | ✅ VERIFIED 2026-07-10 |
| DIAG-TRAIN-003 | flowchart | Corpus pipeline dispatch and unsupported-step boundary | `how-to/training-and-adapters.md` | `hkask-mcp-corpus`, `hkask-types`, `hkask-mcp` | ✅ VERIFIED 2026-07-10 |
| DIAG-TRAIN-004 | flowchart | Full training pipeline (reasoning + Rust adapters + eval) | `how-to/training-and-adapters.md` | HF: `Axolotl-Partners/rust-adapter-scripts` | ✅ VERIFIED 2026-07-11 |
| DIAG-TRAIN-005 | state | Training job lifecycle: Queued → Running → Completed → Terminated | `how-to/training-and-adapters.md` | `hkask-mcp-training/src/providers/types.rs`, HF: `Axolotl-Partners/rust-adapter-scripts` | ✅ VERIFIED 2026-07-11 |
| DIAG-TRAIN-006 | class | Training server type hierarchy: TrainingHost, HarnessAdapter, PodStatus, params | `how-to/training-and-adapters.md` | `hkask-mcp-training/src/providers/{types,runpod,deepinfra,nebius,harness,trl_harness}.rs` | ✅ VERIFIED 2026-07-23 |



## 11. Additional Inlined Diagrams (Not Previously Indexed)

The following diagrams were standalone files not individually tracked in the original index sections 1–10. They were inlined into their parent documents. **Several parent documents have since been deleted (2026-07-24 cleanup); those diagrams are recoverable via git history only.**

| Diagram File (former) | Type | Now Inline In | Description | Status |
|----------------------|------|---------------|-------------|--------|
| `class-ports-trait-hierarchy.md` | class | `explanation/architecture-patterns.md` | Hexagonal Ports Trait Hierarchy | ⚠️ PARENT DELETED |
| `class-service-error-hierarchy.md` | class | `explanation/architecture-patterns.md` | ServiceError Hierarchy | ⚠️ PARENT DELETED |
| `erd-k8s-resources.md` | ERD | `how-to/deployment-and-transport.md` | K8s Resource Relationships | ⚠️ PARENT DELETED |
| `flowchart-architecture-overview.md` | flowchart | `explanation/architecture-patterns.md` | Classification + Guard Architecture Overview | ⚠️ PARENT DELETED |
| `flowchart-regulation-homeostatic-loop.md` | flowchart | `explanation/regulation-and-loops.md` | Regulation Homeostatic Loop | ⚠️ PARENT DELETED |
| `flowchart-regulation-regulation.md` | flowchart | `explanation/regulation-and-loops.md` | Regulation Regulation Pipeline — 5-Phase Cybernetic Cycle | ⚠️ PARENT DELETED |
| `flowchart-curator-metacognition.md` | flowchart | `explanation/regulation-and-loops.md` | Curator Metacognition Loop | ⚠️ PARENT DELETED |
| `flowchart-deployment-architecture.md` | flowchart | `how-to/deployment-and-transport.md` | K8s Deployment Architecture | ⚠️ PARENT DELETED |
| `flowchart-algo-classification.md` | flowchart | `explanation/cognition-and-replica.md` | Algo / No-Judge Classification Flow | ✅ SURVIVES |
| `flowchart-guard-pipeline.md` | flowchart | `explanation/sovereignty-and-ocap.md` | Content Safety Guard Pipeline | ⚠️ PARENT DELETED |
| `flowchart-memory-remember.md` | flowchart | `explanation/cognition-and-replica.md` | Memory Remember — Algo / No-Judge Template Cascade | ✅ SURVIVES |
| `flowchart-oauth-registration.md` | flowchart | `how-to/deployment-and-transport.md` | OAuth Registration & Onboarding Flow | ⚠️ PARENT DELETED |
| `flowchart-pod-startup.md` | flowchart | `how-to/deployment-and-transport.md` | K8s Pod Startup Sequence | ⚠️ PARENT DELETED |
| `sequence-auth-flow.md` | sequence | `how-to/deployment-and-transport.md` | Authentication Flow — OAuth Sequence | ⚠️ PARENT DELETED |
| `sequence-classify-to-memory.md` | sequence | `explanation/cognition-and-replica.md` | Classification-to-Memory Sequence | ✅ SURVIVES |
| `sequence-mcp-bootstrap.md` | sequence | `explanation/architecture-patterns.md` | MCP Bootstrap and Tool Dispatch | ⚠️ PARENT DELETED |
| `state-guard-violations.md` | state | `explanation/sovereignty-and-ocap.md` | Guard Violation Lifecycle | ⚠️ PARENT DELETED |
| `state-invite-lifecycle.md` | state | `how-to/deployment-and-transport.md` | Invite Lifecycle State Machine | ⚠️ PARENT DELETED |
| `state-loop-action-lifecycle.md` | state | `explanation/regulation-and-loops.md` | RegulatoryAction Lifecycle | ⚠️ PARENT DELETED |

## 12. Swarm System Diagrams

The swarm system (`hkask-mcp-swarm` + `crates/swarm_panel` + the `swarm-intelligence` / `swarm-steering` skills) is documented cross-cuttingly in `diagrams/` and the `diataxis/swarm_system/` set. These standalone diagram files survive in `docs/diagrams/` (the directory was not eliminated — the §12 summary note was stale).

| Diagram ID | Type | File | Description | Verified Against | Status |
|-----------|------|------|-------------|------------------|--------|
| DIAG-DIA-SWARM-001 | flowchart | `diagrams/flowchart-swarm-architecture.md` | Swarm MCP server architecture — two launch paths, two substrates, panel + skills + curator | `hkask_mcp_swarm.rs:2822`, `swarm_panel.rs:1870` | ✅ VERIFIED 2026-08-03 (tool count corrected 31→41) |
| DIAG-DIA-SWARM-002 | flowchart | `diagrams/flowchart-swarm-pdca-cascade.md` | swarm-intelligence 10-step PDCA cascade with deterministic compute steps | `swarm-intelligence/SKILL.md:62` | ✅ VERIFIED |
| DIAG-DIA-SWARM-003 | sequence | `diagrams/sequence-swarm-steering-loop.md` | Steering loop — advisory vs steering execution, delegate_results feedback | `swarm-steering/SKILL.md:59` | ✅ VERIFIED |
| DIAG-DIA-SWARM-006 | class | `diagrams/class-swarm-server.md` | SwarmServer collaborators — AbwClient, ConsentStore, SpendGate, runtime, executor, A2A, LocalDelegateResult | `hkask_mcp_swarm.rs:115`, `consent.rs:56`, `spend_gate.rs:83`, `local_runtime.rs:39`, `agent_executor.rs:55`, `a2a.rs:24` | ✅ VERIFIED 2026-08-03 |
| DIAG-DIA-SWARM-007 | state | `diagrams/state-swarm-panel-modes.md` | SwarmPanel PanelMode states (Browse/Author/Compose/Steer) + backend toggle | `swarm_panel.rs:230,289,1798,1834,1870` | ✅ VERIFIED 2026-08-03 |
| DIAG-DIA-SWARM-008 | flowchart | `diagrams/flowchart-swarm-feedback-loops.md` | Four feedback loops with 5-property health + algedonic override + C4 latency deficit | `swarm-intelligence/SKILL.md:62,96,104,122,124,182`, `consent.rs:77,184`, `swarm_panel.rs:191` | ✅ VERIFIED 2026-08-03 |

**Companion audit:** [`audits/swarm-cybernetics-semantics-audit.md`](audits/swarm-cybernetics-semantics-audit.md) — pragmatic-semantics gap analysis + pragmatic-cybernetics per-property loop assessment + VSM map + Ashby variety check.

## 13. Summary

All Mermaid diagrams were inline in their parent documents. The former `docs/diagrams/` directory has been eliminated (all standalone files inlined or deleted as duplicates). **A 2026-07-24 cleanup deleted several parent documents** (`explanation/regulation-and-loops.md`, `explanation/architecture-patterns.md`, `explanation/sovereignty-and-ocap.md`, `explanation/federation-and-transport.md`, `how-to/training-and-adapters.md`, `how-to/deployment-and-transport.md`); diagrams inlined into those files are recoverable via git history only. The Diataxis documentation set (`docs/diataxis/`) added 40 new per-crate diagrams (one per artifact) that are tracked in `diataxis/INDEX.md`, not here.

**Parent document diagram distribution (surviving only):**

| Parent Document | Inlined Diagram Count |
|----------------|----------------------|
| `explanation/cognition-and-replica.md` | 9 |
| `explanation/training-and-adapters.md` | 5 |
| `explanation/skills-and-composition.md` | 3 |
| `reference/mcp-servers/scenarios.md` | 1 |
| `reference/mcp-servers/companies.md` | 1 |
| `reference/mcp-servers/condenser.md` | 1 |
| `reference/mcp-servers/README.md` | 1 |
| `reference/regulation-spans.md` | 1 |
| `architecture/core/MDS.md` | 1 |
| `diataxis/` (40 artifacts, 1 diagram each) | 40 |
| `plans/kask-skill-signing-and-trust.md` | 2 |
| **Total (surviving)** | **65** |

**Deleted parent documents (diagrams recoverable via git):**

| Deleted Parent Document | Diagrams Lost |
|------------------------|---------------|
| `explanation/regulation-and-loops.md` | 8 |
| `explanation/architecture-patterns.md` | 7 |
| `how-to/deployment-and-transport.md` | 6 |
| `explanation/sovereignty-and-ocap.md` | 4 |
| `explanation/federation-and-transport.md` | 1 |
| `how-to/training-and-adapters.md` | (merged into `explanation/training-and-adapters.md`) |
| **Total lost** | **26** |

**MDS completeness:** all five MDS categories have diagram coverage. Training diagrams are additionally anchored to the P2 consent boundary, P4 capability-boundary requirement, and P9 feedback-loop requirement in [`PRINCIPLES.md`](architecture/core/PRINCIPLES.md).

---

## References

[^mds]: hKask Team. (2026). *MDS — Minimal Domain Specification*. `docs/architecture/core/MDS.md`.
[^doc-standards]: hKask Team. (2026). *Documentation Standards*. `docs/specifications/DOCUMENTATION_STANDARDS.md`.

---

*ℏKask v0.32.0 — A Sovereign Chat Client for Human Users with AI Skills — Diagram Verification Registry*
*Mermaid-First Mandate: Every interaction pattern, data flow, and object model is diagrammed.*
*All diagrams inline per DOCUMENTATION_STANDARDS §1 — consolidated 2026-07-12.*