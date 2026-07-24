---
title: "hKask Diagram Index — Mermaid Verification Registry"
audience: [architects, developers, agents]
last_updated: 2026-07-21
version: "0.31.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle, curation]
---

# hKask Diagram Index — Mermaid Verification Registry

**Purpose:** Verifiable registry of all Mermaid diagrams in the hKask documentation corpus. Per the Mermaid-First Mandate from `DOCUMENTATION_STANDARDS.md` §4: every interaction pattern, data flow, and object model is diagrammed. Every diagram carries `DIAGRAM_ALIGNMENT` metadata.

**Consolidation status (2026-07-12):** Most standalone diagram files from the former `docs/diagrams/` directory have been inlined into their parent documents per `DOCUMENTATION_STANDARDS.md` §1. A small number of standalone reference diagrams remain in `docs/diagrams/` for crate-specific architecture flows (e.g., scenario forecasting pipeline, condenser pipeline). This registry maps each diagram to the document where it currently resides.

---

## 1. Domain & Capability Diagrams

| Diagram ID | Description | Now Inline In | Verified Against | Status |
|-----------|-------------|---------------|-----------------|--------|
| DIAG-DC-001 | hKask Bounded Context (POD → CAP → TPL → Regulation) + delegated dependencies | `architecture/core/FUNCTIONAL_SPECIFICATION.md` §1.5.2 | `crates/hkask-pods/src/pod/mod.rs:83`, `crates/hkask-capability/src/lib.rs`, `Cargo.toml` workspace members | ✅ VERIFIED 2026-07-01 |
| DIAG-DC-002 | Domain Entity Map — 9 entities with crate/struct locations | `architecture/core/FUNCTIONAL_SPECIFICATION.md` §4.1 | `crates/hkask-types/src/`, `crates/hkask-pods/src/` | ✅ VERIFIED 2026-07-01 |
| DIAG-DC-003 | Agent Taxonomy (Bot/UserPod branching) | `architecture/core/FUNCTIONAL_SPECIFICATION.md` §4.1 | `crates/hkask-pods/src/pod/types.rs`, `crates/hkask-pods/src/types/agent/definition.rs` | ✅ VERIFIED 2026-07-01 (crate renamed `hkask-pods` → `hkask-pods`) |
| DIAG-DC-004 | OCAP Capability Attenuation Chain (depth ≤ 7) | `explanation/sovereignty-and-ocap.md` | `crates/hkask-capability/src/lib.rs` | ✅ VERIFIED 2026-07-01 |
| DIAG-DC-005 | MCP Tool Dispatch with OCAP constraint enforcement | `explanation/architecture-patterns.md` | `crates/hkask-mcp/src/runtime.rs:59`, `crates/hkask-mcp/src/security.rs` | ✅ VERIFIED 2026-07-01 |
| DIAG-DC-006 | Standing Session Chat Lifecycle | `architecture/core/FUNCTIONAL_SPECIFICATION.md` §1.5.3 | `crates/hkask-cli/src/commands/chat.rs`, `mcp-servers/hkask-mcp-research/src/main.rs` | ✅ VERIFIED 2026-07-01 |
| DIAG-DC-007 | hKask Container Lifecycle (Create → Register → Activate → Deactivate) | `how-to/install-and-configure.md` | `crates/hkask-cli/src/commands/chat.rs`, `crates/hkask-pods/src/pod/mod.rs` | ✅ VERIFIED 2026-07-01 (parent doc consolidated; crate renamed) |
| DIAG-DC-008 | Adapter Lifecycle State Machine (Cold → Warming → Active → Draining → Removed) | `explanation/federation-and-transport.md` | `mcp-servers/hkask-mcp-training/src/adapter/endpoint_lifecycle.rs`, `mcp-servers/hkask-mcp-training/src/adapter/adapter_router/mod.rs` | ✅ VERIFIED 2026-07-01 |
| DIAG-DC-009 | CodeGraph Type System — Symbol, Edge, GraphStore, IndexPipeline, 10-tool MCP server | `reference/api-reference.md` | `mcp-servers/hkask-mcp-codegraph/src/codegraph/types.rs`, `mcp-servers/hkask-mcp-codegraph/src/codegraph/graph/store.rs`, `mcp-servers/hkask-mcp-codegraph/src/codegraph/indexer/pipeline.rs`, `mcp-servers/hkask-mcp-codegraph/src/lib.rs` | ✅ VERIFIED 2026-07-04 |
| DIAG-DC-010 | CodeGraph Indexing Pipeline — SHA-256 hash → tree-sitter parse → extract → insert → rank | `reference/api-reference.md` | `mcp-servers/hkask-mcp-codegraph/src/codegraph/indexer/pipeline.rs`, `mcp-servers/hkask-mcp-codegraph/src/codegraph/indexer/extractor.rs`, `mcp-servers/hkask-mcp-codegraph/src/codegraph/graph/store.rs` | ✅ VERIFIED 2026-07-04 |
| DIAG-DC-011 | CodeGraph Database Schema — 3 tables, 2 virtual tables, FTS5 triggers, WAL mode | `reference/api-reference.md` | `mcp-servers/hkask-mcp-codegraph/src/codegraph/graph/schema.rs:26-126` | ✅ VERIFIED 2026-07-04 |
| DIAG-DC-012 | Research Compound Search Flow — validate → cache → strategy → join_all → RRF fusion → rerank → deep extract → record | (removed - host status report deleted; recoverable via git) | `mcp-servers/hkask-mcp-research/src/research/providers/mod.rs:213-410,516-620`, `mcp-servers/hkask-mcp-research/src/lib.rs:265-375` | ✅ VERIFIED 2026-07-17 |
| DIAG-DC-013 | CodeGraph Architecture — CodeGraphServer, indexed_once flag, IndexPipeline, GraphStore, EmbeddingRouter, Jinja | (removed - host status report deleted; recoverable via git) | `mcp-servers/hkask-mcp-codegraph/src/lib.rs:24-31,33-76,159-548`, `mcp-servers/hkask-mcp-codegraph/src/codegraph/lib.rs:20-31`, `mcp-servers/hkask-mcp-codegraph/src/codegraph/indexer/pipeline.rs:22-273`, `mcp-servers/hkask-mcp-codegraph/src/codegraph/graph/mod.rs:1-7` | ✅ VERIFIED 2026-07-20 |

## 2. Interface & Composition Diagrams

| Diagram ID | Description | Now Inline In | Verified Against | Status |
|-----------|-------------|---------------|-----------------|--------|
| DIAG-IC-001 | MCP ≡ CLI ≡ API Equivalence Model | `architecture/core/FUNCTIONAL_SPECIFICATION.md` §1.5.2 | `crates/hkask-cli/src/cli/mod.rs:33`, `crates/hkask-api/src/lib.rs:317`, `crates/hkask-mcp/src/runtime.rs:59` | ✅ VERIFIED 2026-07-01 |
| DIAG-IC-002 | Hexagonal Architecture — Ports, Adapters, Core | `explanation/architecture-patterns.md` | `crates/hkask-types/src/` (7 port traits) | ✅ VERIFIED 2026-07-01 |
| DIAG-IC-003 | Unified Registry with template_type discriminator | `architecture/core/FUNCTIONAL_SPECIFICATION.md` §4.3 | `crates/hkask-templates/src/` (SqliteRegistry) | ✅ VERIFIED 2026-07-01 |
| DIAG-IC-004 | Template Cascade Flow (depth ≤ 7, DependencyGraph acyclic) | `explanation/architecture-patterns.md` | `crates/hkask-templates/src/executor.rs` | ✅ VERIFIED 2026-07-01 |
| DIAG-IC-005 | Rendering Pipeline — Template → Jinja2 → LLM | `explanation/architecture-patterns.md` | `crates/hkask-templates/src/` (minijinja integration) | ✅ VERIFIED 2026-07-01 |
| DIAG-IC-006 | LLM Routing and Failover (Inference Router — DI/TG/FA/OR) | `architecture/core/FUNCTIONAL_SPECIFICATION.md` §2.5 | `crates/hkask-mcp/src/runtime.rs`, `crates/hkask-mcp/src/security.rs` | ✅ VERIFIED 2026-07-01 |
| DIAG-IC-007 | MCP Tool Dispatch Sequence with OCAP Enforcement | `explanation/architecture-patterns.md` | `crates/hkask-mcp/src/runtime.rs`, `crates/hkask-mcp/src/security.rs` | ✅ VERIFIED 2026-07-01 |
| DIAG-IC-008 | Service Layer Decomposition — 10 subcrates, ports, CLI/API consumers | `explanation/architecture-patterns.md` | `crates/hkask-services-core through hkask-services-wallet/src/`, `crates/hkask-types/src/` | ✅ VERIFIED 2026-07-12 |
| DIAG-IC-009 | CodeGraph Agent Workflow — search, traverse, impact, context assembly, feedback loop | `reference/api-reference.md` | `mcp-servers/hkask-mcp-codegraph/src/lib.rs:243-632`, `mcp-servers/hkask-mcp-codegraph/src/codegraph/graph/search.rs`, `mcp-servers/hkask-mcp-codegraph/src/codegraph/graph/traversal.rs` | ✅ VERIFIED 2026-07-04 |
| DIAG-IC-010 | Companies provider routing — symbol selection, learning override, fallback, EODHD normalization | `architecture/core/hKask-architecture-master.md` | `mcp-servers/hkask-mcp-companies/src/providers.rs:84-247`, `mcp-servers/hkask-mcp-companies/src/lib.rs:340-361` | ✅ VERIFIED 2026-07-10 |
| DIAG-IC-011 | Companies forecast feedback — durable snapshot, revision, outcome, and daemon experience flow | `architecture/core/hKask-architecture-master.md` | `mcp-servers/hkask-mcp-companies/src/tools/analytics.rs:438-457`, `mcp-servers/hkask-mcp-companies/src/tools/valuation.rs:634-659,774-915`, `mcp-servers/hkask-mcp-companies/src/portfolio.rs:303-400` | ✅ VERIFIED 2026-07-10 |
| DIAG-IC-012 | Regulation Architecture — responsibility clusters, wallet port, extraction status | `explanation/regulation-and-loops.md` | `crates/hkask-regulation/src/cybernetics_loop.rs`, `crates/hkask-regulation/src/runtime.rs`, `crates/hkask-regulation/src/wallet_budget.rs`, `crates/hkask-regulation/src/slo_manager.rs`, `crates/hkask-services-context/src/storage_guard.rs`, `crates/hkask-regulation/src/seam_watcher.rs`, `crates/hkask-types/src/wallet_budget_port.rs` | ✅ VERIFIED 2026-07-11 |
| DIAG-IC-013 | Research MCP Server Architecture — ResearchServer, ProviderPool, WebSearchPort, cache, rate limiter, RSS DB | (removed - host status report deleted; recoverable via git) | `mcp-servers/hkask-mcp-research/src/lib.rs:41-48`, `mcp-servers/hkask-mcp-research/src/research/providers/mod.rs:130-135,494-620` | ✅ VERIFIED 2026-07-17 |
| DIAG-IC-014 | Research Provider Trait Hierarchy — WebSearchPort, WebSearchProvider, WebExtractProvider, WebBrowseProvider, 9 concrete providers | (removed - host status report deleted; recoverable via git) | `mcp-servers/hkask-mcp-research/src/research/providers/mod.rs:50-135`, `mcp-servers/hkask-mcp-research/src/research/providers/brave.rs:18`, `mcp-servers/hkask-mcp-research/src/research/providers/firecrawl.rs:28,100,181` | ✅ VERIFIED 2026-07-17 |
| DIAG-IC-015 | Skill MCP Server Architecture — SkillServer stores RegistryEntry (lazy read), templates-vs-skills distinction, InferencePort | `reference/mcp-servers/skill-server.md` | `mcp-servers/hkask-mcp-skill/src/lib.rs:49`, `crates/hkask-templates/src/registry.rs:400` | ✅ VERIFIED 2026-07-17 |
| DIAG-IC-016 | CodeGraph Tool Dispatch Flow — execute_tool → ensure_indexed (indexed_once check) → lock pipeline → graph operation → JSON response | (removed - host status report deleted; recoverable via git) | `mcp-servers/hkask-mcp-codegraph/src/lib.rs:34-76,163-181,431-455`, `mcp-servers/hkask-mcp-codegraph/src/codegraph/indexer/pipeline.rs:61-159,245-263` | ✅ VERIFIED 2026-07-20 |
| DIAG-IC-017 | Kata-Kanban MCP Server Architecture — KanbanServer, KanbanService, KataEngine, HMemStore, Task, Board, TaskStatus, SocraticRole class relationships | `reference/mcp-servers/README.md` (Kata-Kanban Server Architecture section) | `mcp-servers/hkask-mcp-kata-kanban/src/lib.rs:29-33`, `crates/hkask-services-kata-kanban/src/kanban/service_impl/service.rs:34-37`, `crates/hkask-services-kata-kanban/src/kata/mod.rs:76-94`, `crates/hkask-storage/src/hmem.rs:134-138` | ✅ VERIFIED 2026-07-20 (inlined from `diagrams/class-kata-kanban-architecture.md`) |

## 3. Trust & Observability Diagrams

| Diagram ID | Description | Now Inline In | Verified Against | Status |
|-----------|-------------|---------------|-----------------|--------|
| DIAG-TO-001 | STRIDE-lite Threat Model (4 adversaries) | `architecture/core/FUNCTIONAL_SPECIFICATION.md` §2.4 | `crates/hkask-mcp/src/security.rs`, `crates/hkask-keystore/src/` | ✅ VERIFIED 2026-07-01 |
| DIAG-TO-002 | OCAP Boundary Enforcement Flow | `explanation/sovereignty-and-ocap.md` | `crates/hkask-mcp/src/security.rs` (SecurityGateway) | ✅ VERIFIED 2026-07-01 |
| DIAG-TO-003 | Encryption Stack — Argon2id → AES-256-GCM → SQLCipher | `architecture/core/hKask-architecture-master.md` | `crates/hkask-keystore/src/`, `crates/hkask-storage/src/database.rs` | ✅ VERIFIED 2026-07-01 |
| DIAG-TO-004 | Regulation Span Emission Flow (4 namespaces → Sink) | `explanation/regulation-and-loops.md` | `crates/hkask-regulation/src/runtime.rs`, `crates/hkask-types/src/event.rs` | ✅ VERIFIED 2026-07-01 |
| DIAG-TO-005 | Algedonic Alert Escalation (variety deficit > threshold → Curator/Human) | `explanation/regulation-and-loops.md` | `crates/hkask-regulation/src/algedonic.rs` | ✅ VERIFIED 2026-07-01 |
| DIAG-TO-006 | Regulation Span Emission and Algedonic Alert End-to-End Flow | `explanation/regulation-and-loops.md` | `crates/hkask-pods/src/curator_agent/spec_curator.rs`, `crates/hkask-regulation/src/cybernetics_loop.rs`, `crates/hkask-regulation/src/algedonic.rs` | ✅ VERIFIED 2026-07-01 (crate renamed) |
| DIAG-TO-006-CM | ConsentManager Authorization Flow | `explanation/sovereignty-and-ocap.md` | `crates/hkask-pods/src/consent.rs`, `crates/hkask-pods/src/sovereignty.rs`, `crates/hkask-storage/src/consent_store.rs` | ✅ VERIFIED 2026-07-01 |

## 4. Persistence & Lifecycle Diagrams

| Diagram ID | Description | Now Inline In | Verified Against | Status |
|-----------|-------------|---------------|-----------------|--------|
| DIAG-PL-001 | Database Architecture — SQLCipher with 9 specialized stores | `architecture/core/hKask-architecture-master.md` | `crates/hkask-storage/src/database.rs:74` | ✅ VERIFIED 2026-07-01 |
| DIAG-PL-002 | Bitemporal hMem Schema (valid-time × transaction-time) | `architecture/core/hKask-architecture-master.md` | `crates/hkask-storage/src/triples.rs:79` | ✅ VERIFIED 2026-07-01 |
| DIAG-PL-003 | Memory Architecture — Episodic/Semantic public/private gating | `explanation/cognition-and-replica.md` | `crates/hkask-memory/src/` | ✅ VERIFIED 2026-07-01 |
| DIAG-PL-004 | Bootstrap Sequence (superseded by DIAG-PL-006) | `how-to/install-and-configure.md` | `crates/hkask-cli/src/main.rs` (superseded) | ⚠️ SUPERSEDED by DIAG-PL-006 |
| DIAG-PL-005 | Embedding Vector Lifecycle (model → sqlite-vec → KNN search) | `architecture/core/hKask-architecture-master.md` | `crates/hkask-storage/src/embeddings.rs` | ✅ VERIFIED 2026-07-01 |
| DIAG-PL-006 | Bootstrap Flowchart — full CLI entry → AgentService assembly → surface mount | `how-to/install-and-configure.md` | `crates/hkask-cli/src/main.rs`, `crates/hkask-services-context/src/context_impl/build/`, `crates/hkask-services-core/src/config.rs`, `crates/hkask-api/src/lib.rs` | ✅ VERIFIED 2026-07-01 |
| DIAG-PL-010 | Database Schema ERD — 37 tables, 16 relationships, full Crow's Foot notation | `architecture/core/hKask-architecture-master.md` | `crates/hkask-storage/src/sql/schema.sql`, `crates/hkask-storage/src/sql/users.sql`, `crates/hkask-storage/src/*.rs` | ✅ VERIFIED 2026-07-01 |
| DIAG-PL-011 | CodeGraph Indexing Pipeline — walk → hash → parse → extract → insert → rank → FTS5 | `reference/api-reference.md` | `mcp-servers/hkask-mcp-codegraph/src/codegraph/indexer/pipeline.rs`, `mcp-servers/hkask-mcp-codegraph/src/codegraph/indexer/extractor.rs`, `mcp-servers/hkask-mcp-codegraph/src/codegraph/graph/store.rs` | ✅ VERIFIED 2026-07-04 |
| DIAG-PL-012 | CodeGraph Pipeline Lifecycle — Uninitialized → Indexing → Ready → Stale | `reference/api-reference.md` | `mcp-servers/hkask-mcp-codegraph/src/codegraph/indexer/pipeline.rs:38-273` | ✅ VERIFIED 2026-07-04 |

## 5. Framework & Methodology Diagrams

| Diagram ID | Description | Now Inline In | Verified Against | Status |
|-----------|-------------|---------------|-----------------|--------|
| DIAG-FW-001 | MDS RDF/Turtle Semantic Graph | `architecture/core/MDS.md` §1.1 | `docs/architecture/core/MDS.md` (textual RDF reference) | ✅ VERIFIED 2026-07-01 |
| DIAG-FW-002 | MDS Entity Relationship Diagram (Spec ↔ Goal ↔ Curation) | `architecture/core/MDS.md` §1.2 | `docs/architecture/core/MDS.md` (textual ERD reference) | ✅ VERIFIED 2026-07-01 |
| DIAG-FW-003 | MVSDD Cycle Sequence Diagram (Specify → Grant → Compose → Curate → Reflect) | `architecture/core/MDS.md` §4.3 | `docs/architecture/core/MDS.md` (textual cycle reference) | ✅ VERIFIED 2026-07-01 |
| DIAG-FW-004 | Hexagonal Component Diagram (HKaskHexagon) | `explanation/architecture-patterns.md` | `crates/hkask-types/src/` | ✅ VERIFIED 2026-07-01 |
| DIAG-FW-005 | Kata PDCA State Machine — Plan → Do → Check → Act with Kanban integration | `how-to/skills-and-composition.md` | `crates/hkask-services-kata-kanban/src/kata/`, `crates/hkask-services-kata-kanban/src/kanban/` | ✅ VERIFIED 2026-07-01 |
| DIAG-FW-006 | Kata-Kanban execution boundary — MCP prompt generation vs CLI full Kata execution | `how-to/skills-and-composition.md` | `mcp-servers/hkask-mcp-kata-kanban/src/lib.rs:656-686`, `crates/hkask-services-kata-kanban/src/kanban/service_impl/kata.rs:104-177`, `crates/hkask-services-kata-kanban/src/kata/mod.rs:334-498`, `crates/hkask-cli/src/commands/kata.rs:225-287` | ✅ VERIFIED 2026-07-20 |
| DIAG-FW-007 | Scenario forecasting pipeline — framing, reviewed events, computation, calibration, assessment | `architecture/core/hKask-architecture-master.md` | `mcp-servers/hkask-mcp-scenarios/src/lib.rs:459-1708`, `mcp-servers/hkask-mcp-scenarios/src/superforecast.rs:165-400` | ✅ VERIFIED 2026-07-10 |
| DIAG-FW-008 | Kanban Task Lifecycle State Diagram — Backlog → Ready → InProgress → Review → Done with reopen and gas-exhaust transitions | `how-to/skills-and-composition.md` (Kanban Task Lifecycle State Machine section) | `crates/hkask-services-kata-kanban/src/kanban/types/status.rs:61-73`, `crates/hkask-services-kata-kanban/src/kanban/service_impl/service.rs:814-837`, `crates/hkask-services-kata-kanban/src/kanban/service_impl/dejam.rs:180-207` | ✅ VERIFIED 2026-07-20 (inlined from `diagrams/state-kanban-task-lifecycle.md`) |

## 6. Reference Diagrams

| Diagram ID | Description | Now Inline In | Verified Against | Status |
|-----------|-------------|---------------|-----------------|--------|
| DIAG-RF-001 | ERD Schema Documentation (superseded by DIAG-PL-010) | `architecture/core/hKask-architecture-master.md` | `crates/hkask-storage/src/` (retained as historical reference; canonical is DIAG-PL-010) | ✅ VERIFIED 2026-07-01 |
| DIAG-RF-002 | Condenser Pipeline — tool dispatch, algorithm learning (auto-selection), two-phase condensation (CPU + LLM), ontology anchoring, profile suggestion | `reference/mcp-servers/condenser.md` | `mcp-servers/hkask-mcp-condenser/src/lib.rs`, `crates/hkask-condenser/src/engine.rs`, `crates/hkask-condenser/src/algorithms.rs`, `crates/hkask-services-chat/src/chat/condenser.rs` | ✅ VERIFIED 2026-07-21 (inlined from `diagrams/flowchart-condenser-pipeline.md`) |
| DIAG-RF-003 | Filesystem sandbox path resolution + tool dispatch flow (execute_tool → sandbox_path → canonicalize → containment → emit_reg) | `reference/mcp-servers/filesystem.md` | `mcp-servers/hkask-mcp-filesystem/src/lib.rs:55-109`, `crates/hkask-mcp/src/server/tool_span.rs:246-259` | ✅ VERIFIED 2026-07-17 |
| DIAG-DOC-001 | hKask Documentation Structure — Diataxis navigation map (quadrants + supporting directories) | `README.md` (Diataxis Structure section) | `docs/README.md`, `docs/specifications/DOCUMENTATION_STANDARDS.md`, `docs/` directory listing | ✅ VERIFIED 2026-07-21 (inlined from `diagrams/flowchart-documentation-structure.md`) |
| DIAG-RF-004 | Companies tool routing and dispatch flow — combined_router (7 sub-routers) → execute_tool seam → three sinks (provider fetch, valuation engines → StoredForecast, PortfolioManager spawn_blocking) | `reference/mcp-servers/companies.md` | `mcp-servers/hkask-mcp-companies/src/lib.rs:499-509,368-495`, `mcp-servers/hkask-mcp-companies/src/tools/mod.rs:1-8`, `mcp-servers/hkask-mcp-companies/src/providers.rs:111-198`, `mcp-servers/hkask-mcp-companies/src/portfolio.rs:290-340` | ✅ VERIFIED 2026-07-17 (standalone duplicate deleted) |
| DIAG-RF-005 | Scenario Forecasting Pipeline — 18 MCP tools grouped by pipeline phase (Framing → Ideation → Structuring → Computation → Aggregation → Tracking → Assessment) with engine delegation | `reference/mcp-servers/scenarios.md` | `mcp-servers/hkask-mcp-scenarios/src/lib.rs`, `mcp-servers/hkask-mcp-scenarios/src/superforecast.rs`, `mcp-servers/hkask-mcp-scenarios/src/types.rs` | ✅ VERIFIED 2026-07-21 (inlined from `diagrams/flowchart-scenario-forecasting-pipeline.md`) |
| DIAG-REPL-001 | REPL Turn Pipeline — control flow of `run_turn_loop()` (gas reserve → execute → extract tools → invoke → feed back) | `specifications/REPL-specification.md` §6.1 | `crates/hkask-repl/src/turn.rs:130-307` | ✅ VERIFIED 2026-07-20 (inlined from `diagrams/flowchart-repl-turn-pipeline.md`) |
| DIAG-REPL-002 | ReplState Decomposition — type hierarchy of `ReplState` and sub-structs (`ToolPrompt`, `ManifestCascade`, `TalkConfig`, `TalkMode`, `ThreadRegistry`, `ReplHost`) | `specifications/REPL-specification.md` §3.2 | `crates/hkask-repl/src/lib.rs:100-159`, `crates/hkask-services-context/src/context_impl.rs:103-474` | ✅ VERIFIED 2026-07-20 (inlined from `diagrams/class-repl-state-decomposition.md`) |
| DIAG-REPL-003 | REPL Tool Invocation — sequence diagram of OCAP boundary (DelegationToken → GovernedTool → RawMcpToolPort → McpRuntime → MCP server) | `specifications/REPL-specification.md` §6.3 | `crates/hkask-repl/src/deps.rs:262-293`, `crates/hkask-regulation/src/governed_tool.rs` | ✅ VERIFIED 2026-07-20 (inlined from `diagrams/sequence-repl-tool-invocation.md`; stale `tool_augmented.rs` ref removed) |

## 7. Undocumented Interaction Patterns (V1.1+ Candidates)

These interaction patterns exist in the codebase but lack dedicated diagram coverage. They are candidates for v1.1+ diagram work.

| Pattern | MDS Category | Crates Involved | Priority |
|---------|----------------|----------------|----------|
| Federation Message Flow (deferred) | Composition | `hkask-*` (deferred to v1.1+) | P2 |
| Competition Socket Protocol (ACP) | Interface | `hkask-pods` (ACP) | P2 |
| Git CAS Content-Addressed Blob Flow | Persistence | `hkask-storage (git_cas)`, `gix 0.81` | P2 |
| Template Manifest Validation Flow (ContractValidator) | Composition | `hkask-templates` | P2 |
| MVSDD Cycle (Specify → Grant → Compose → Curate → Reflect) | Curation | `hkask-templates`, `hkask-pods` | P2 |

> **Note (2026-06-09):** `hkask-mcp-memory` consolidates episodic and semantic memory operations. Its interaction patterns with the memory subsystem are now covered by DIAG-PL-003 (inlined in `explanation/cognition-and-replica.md`).

---

## 8. FUNCTIONAL_SPECIFICATION.md — Inline Mermaid Diagrams

The `docs/architecture/core/FUNCTIONAL_SPECIFICATION.md` contains 14 inline Mermaid ERD and flowchart diagrams covering gas budgeting, governance, runtime observability, deployment, and entity models. These were always inline (never standalone) and are cross-referenced in §1–§4 above.

| Diagram ID | Description | Section | Diagram Type |
|-----------|-------------|---------|-------------|
| DIAG-FS-001 | Service Layer Architecture — CLI/API → Service Subcrates → Domain Crates | §1.5.2 | Flowchart (graph TD) |
| DIAG-FS-002 | Loop Architecture Membrane — Domain Loops (Inference, Memory) + Curation + Cybernetics + Transport | §1.5.3 | Flowchart (graph TD) |
| DIAG-FS-003 | GasBudget ERD — Energy Budgeting with OCAP/P9 constraints | §2.1 | ERD (erDiagram) |
| DIAG-FS-004 | AlgedonicManager ERD — Algedonic Signalling with VarietyCounter, CurationLoop | §2.2 | ERD (erDiagram) |
| DIAG-FS-005 | RegulationLedger ERD — Runtime Observability with VarietyMonitor, OutcomeTracker | §2.3 | ERD (erDiagram) |
| DIAG-FS-006 | GovernedTool ERD — Tool Governance with OCAP, Consent, GasBudget constraints | §2.4 | ERD (erDiagram) |
| DIAG-FS-007 | GovernedInference ERD — Inference Governance with CompositeEnergyEstimator | §2.5 | ERD (erDiagram) |
| DIAG-FS-008 | CircuitBreaker ERD — Failure/Success/HalfOpen state tracking | §2.6 | ERD (erDiagram) |
| DIAG-FS-009 | ApiMeter ERD — Rate-Limit Buckets with per-key TokenTracker | §2.7 | ERD (erDiagram) |
| DIAG-FS-010 | CompositeEnergyEstimator ERD — Multi-backend Energy Estimation | §2.8 | ERD (erDiagram) |
| DIAG-FS-011 | CloudServer ERD — Deployment Domain (Caddy, Conduit, UserSession, Wallet) | §3.18 | ERD (erDiagram) |
| DIAG-FS-012 | Core Domain Entity Model — Full entity map (HumanUser, UserPod, Wallet, Session, etc.) | §4.1 | ERD (erDiagram) |
| DIAG-FS-013 | Deployment Domain Entity Model — KaskBinary, ServerProfile, deployment infra | §4.2 | ERD (erDiagram) |
| DIAG-FS-014 | Contract-Anchoring ERD — Principles ↔ Contracts ↔ Sub-Contracts | §4.3 | ERD (erDiagram) |

---

## 9. Training and Corpus Diagrams

| Diagram ID | Type | Description | Now Inline In | Verified Against | Status |
|------------|------|-------------|---------------|------------------|--------|
| DIAG-TRAIN-001 | flowchart | Unsloth Qwen3.6-27B training pipeline | `how-to/training-and-adapters.md` | HF: `Axolotl-Partners/rust-adapter-scripts` | ✅ VERIFIED 2026-07-10 |
| DIAG-TRAIN-002 | flowchart | Replica, corpus, and training readiness boundary | `how-to/training-and-adapters.md` | `hkask-mcp-replica`, `hkask-mcp-docproc`, `hkask-mcp-training` | ✅ VERIFIED 2026-07-10 |
| DIAG-TRAIN-003 | flowchart | Replica pipeline dispatch and unsupported-step boundary | `how-to/training-and-adapters.md` | `hkask-mcp-replica`, `hkask-types`, `hkask-mcp` | ✅ VERIFIED 2026-07-10 |
| DIAG-TRAIN-004 | flowchart | Full training pipeline (reasoning + Rust adapters + eval) | `how-to/training-and-adapters.md` | HF: `Axolotl-Partners/rust-adapter-scripts` | ✅ VERIFIED 2026-07-11 |
| DIAG-TRAIN-005 | state | Training job lifecycle: Queued → Running → Completed → Terminated | `how-to/training-and-adapters.md` | `hkask-mcp-training/src/providers/types.rs`, HF: `Axolotl-Partners/rust-adapter-scripts` | ✅ VERIFIED 2026-07-11 |
| DIAG-TRAIN-006 | class | Training server type hierarchy: TrainingHost, HarnessAdapter, PodStatus, params | `how-to/training-and-adapters.md` | `hkask-mcp-training/src/providers/{types,runpod,deepinfra,nebius,harness,trl_harness}.rs` | ✅ VERIFIED 2026-07-23 |

## 10. TUI (Terminal UI) Diagrams

| Diagram ID | Type | Description | Now Inline In | Verified Against | Status |
|------------|------|-------------|---------------|------------------|--------|
| DIAG-TUI-001 | class | Window trait hierarchy, bridge traits, 22 window types | `reference/api-reference.md` | `crates/hkask-repl/src/tui/window.rs`, `bridges/`, `mcp_tabbed.rs`, `window_catalog.rs` | ✅ VERIFIED 2026-07-20 |
| DIAG-TUI-002 | flowchart | Event dispatch pipeline: crossterm → global → palette → window | `reference/api-reference.md` | `crates/hkask-repl/src/tui/lib.rs:126-221`, `workspace.rs:541-635` | ✅ VERIFIED 2026-07-20 |
| DIAG-TUI-003 | state | Workspace lifecycle: init → splash → running → splits → quit | `reference/api-reference.md` | `crates/hkask-repl/src/tui/workspace.rs`, `layout.rs`, `window.rs` | ✅ VERIFIED 2026-07-20 |
| DIAG-TUI-004 | flowchart | Bridge wiring: CLI → `with_bridges!` → `WorkspaceBridges` → `create_window` → window | `reference/api-reference.md` | `crates/hkask-repl/src/tui/bridges/mod.rs`, `window_catalog.rs`, `crates/hkask-repl/src/tui_bridges.rs` | ✅ VERIFIED 2026-07-20 |
| DIAG-TUI-005 | sequence | Runtime boundary: CLI → REPL assembly → bridge injection → workspace/window → service | `explanation/tui-architecture.md` | `crates/hkask-cli/src/commands/tui.rs`, `crates/hkask-repl/src/lib.rs`, `crates/hkask-repl/src/tui/lib.rs`, `workspace.rs`, `window_catalog.rs` | ✅ VERIFIED 2026-07-20 |

## 11. Additional Inlined Diagrams (Not Previously Indexed)

The following diagrams were standalone files not individually tracked in the original index sections 1–10. They are now inlined in their parent documents.

| Diagram File (former) | Type | Now Inline In | Description |
|----------------------|------|---------------|-------------|
| `class-database-driver.md` | class | `architecture/core/hKask-architecture-master.md` | Database Driver Class Diagram |
| `class-ports-trait-hierarchy.md` | class | `explanation/architecture-patterns.md` | Hexagonal Ports Trait Hierarchy |
| `class-service-error-hierarchy.md` | class | `explanation/architecture-patterns.md` | ServiceError Hierarchy |
| `erd-k8s-resources.md` | ERD | `how-to/deployment-and-transport.md` | K8s Resource Relationships |
| `erd-multi-user.md` | ERD | `architecture/core/hKask-architecture-master.md` | Multi-User Data Model |
| `erd-sqlcipher-schema.md` | ERD | `architecture/core/hKask-architecture-master.md` | SQLCipher Schema |
| `flowchart-architecture-overview.md` | flowchart | `explanation/architecture-patterns.md` | Classification + Guard Architecture Overview |
| `flowchart-connection-lifecycle.md` | flowchart | `architecture/core/hKask-architecture-master.md` | Database Connection Lifecycle |
| `flowchart-regulation-homeostatic-loop.md` | flowchart | `explanation/regulation-and-loops.md` | Regulation Homeostatic Loop |
| `flowchart-regulation-regulation.md` | flowchart | `explanation/regulation-and-loops.md` | Regulation Regulation Pipeline — 5-Phase Cybernetic Cycle |
| `flowchart-curator-metacognition.md` | flowchart | `explanation/regulation-and-loops.md` | Curator Metacognition Loop |
| `flowchart-deployment-architecture.md` | flowchart | `how-to/deployment-and-transport.md` | K8s Deployment Architecture |
| `flowchart-algo-classification.md` | flowchart | `explanation/cognition-and-replica.md` | Algo / No-Judge Classification Flow |
| `flowchart-guard-pipeline.md` | flowchart | `explanation/sovereignty-and-ocap.md` | Content Safety Guard Pipeline |
| `flowchart-memory-remember.md` | flowchart | `explanation/cognition-and-replica.md` | Memory Remember — Algo / No-Judge Template Cascade |
| `flowchart-oauth-registration.md` | flowchart | `how-to/deployment-and-transport.md` | OAuth Registration & Onboarding Flow |
| `flowchart-pod-startup.md` | flowchart | `how-to/deployment-and-transport.md` | K8s Pod Startup Sequence |
| `sequence-auth-flow.md` | sequence | `how-to/deployment-and-transport.md` | Authentication Flow — OAuth Sequence |
| `sequence-classify-to-memory.md` | sequence | `explanation/cognition-and-replica.md` | Classification-to-Memory Sequence |
| `sequence-mcp-bootstrap.md` | sequence | `explanation/architecture-patterns.md` | MCP Bootstrap and Tool Dispatch |
| `state-guard-violations.md` | state | `explanation/sovereignty-and-ocap.md` | Guard Violation Lifecycle |
| `state-invite-lifecycle.md` | state | `how-to/deployment-and-transport.md` | Invite Lifecycle State Machine |
| `state-loop-action-lifecycle.md` | state | `explanation/regulation-and-loops.md` | RegulatoryAction Lifecycle |

## 12. Summary

All Mermaid diagrams are now inline in their parent documents. The former `docs/diagrams/` directory has been eliminated (all 8 standalone files inlined or deleted as duplicates). 72 diagram artifacts total: 57 formerly standalone diagrams inlined into 11 parent documents + 14 inline diagrams in `FUNCTIONAL_SPECIFICATION.md` + 1 newly authored inline diagram (DIAG-RF-003, filesystem sandbox model). Post-pivot cleanup (2026-07-21) inlined 8 more diagrams into 5 parent documents and created 2 new reference files (`condenser.md`, `scenarios.md`).

**Parent document diagram distribution:**

| Parent Document | Inlined Diagram Count |
|----------------|----------------------|
| `explanation/regulation-and-loops.md` | 8 |
| `explanation/architecture-patterns.md` | 7 |
| `reference/api-reference.md` | 9 |
| `architecture/core/hKask-architecture-master.md` | 8 |
| `how-to/training-and-adapters.md` | 6 |
| `how-to/deployment-and-transport.md` | 6 |
| `explanation/cognition-and-replica.md` | 4 |
| `explanation/sovereignty-and-ocap.md` | 4 |
| `how-to/skills-and-composition.md` | 2 |
| `explanation/federation-and-transport.md` | 1 |
| `how-to/install-and-configure.md` | 2 |
| `reference/mcp-servers/filesystem.md` | 1 |
| `architecture/core/FUNCTIONAL_SPECIFICATION.md` | 14 (always inline) |
| **Total** | **72** |

**MDS completeness:** all five MDS categories have diagram coverage. Training diagrams are additionally anchored to the P2 consent boundary, P4 capability-boundary requirement, and P9 feedback-loop requirement in [`PRINCIPLES.md`](architecture/core/PRINCIPLES.md).

---

## References

[^mds]: hKask Team. (2026). *MDS — Minimal Domain Specification*. `docs/architecture/core/MDS.md`.
[^doc-standards]: hKask Team. (2026). *Documentation Standards*. `docs/specifications/DOCUMENTATION_STANDARDS.md`.

---

*ℏKask v0.31.0 — A Sovereign Chat Client for Human Users with AI Skills — Diagram Verification Registry*
*Mermaid-First Mandate: Every interaction pattern, data flow, and object model is diagrammed.*
*All diagrams inline per DOCUMENTATION_STANDARDS §1 — consolidated 2026-07-12.*