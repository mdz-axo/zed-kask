---
title: "hKask Diagram Index — Mermaid Verification Registry"
audience: [architects, developers, agents]
last_updated: 2026-08-28
version: "2.0.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [curation, composition]
---

# hKask Diagram Index — Mermaid Verification Registry

The standalone diagram files under `docs/diagrams/` were consolidated on
2026-08-28 into five domain files. Each consolidated file preserves the
unique `DIAGRAM_ALIGNMENT` IDs of its folded sources; every diagram was
re-verified against current code on consolidation (corrections are noted
per-section in each file). Git history is the archive of record for the
folded originals.

**Memory diagrams** (`erd-memory-store`, `flowchart-memory-recall`,
`sequence-memory-ingest`) were folded into
[`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md)
— they no longer live in this directory.

## 1. Consolidated diagram files

| File | Domain | Diagrams |
|------|--------|----------|
| [`diagrams/architecture.md`](./diagrams/architecture.md) | Cross-cutting | CMP research pipeline, constraint-forces skills, ontology bridge, skill/MCP/lisp seam, credential resolution, tool port, event store, viz-core |
| [`diagrams/kanban.md`](./diagrams/kanban.md) | Kanban | Task status lifecycle, move controller |
| [`diagrams/swarm.md`](./diagrams/swarm.md) | Swarm | Server architecture, server class, panel modes, feedback loops, PDCA cascade, steering loop |
| [`diagrams/ui-widgets.md`](./diagrams/ui-widgets.md) | Viz widgets (D18) | Graph, kanban, portfolio, prediction-markets server, scenarios, swarm widgets |
| [`diagrams/mcp-dispatch.md`](./diagrams/mcp-dispatch.md) | MCP dispatch | Runtime invoke flow, tool-call sequence, CMP tool flow |

## 2. DIAGRAM_ID → location mapping

All preserved diagram IDs, in ID order. Every entry is `VERIFIED` as of
2026-08-28.

| DIAGRAM_ID | Location | Subject |
|------------|----------|---------|
| DIAG-ARCH-SKILL-MCP-LISP-001 | architecture.md | Skill ↔ MCP ↔ Lisp capabilities seam (11 MCP servers) |
| DIAG-CAP-001 | architecture.md | `hkask-tool-port` class diagram |
| DIAG-CAP-002 | mcp-dispatch.md | `McpRuntime::invoke` metering and dispatch flow |
| DIAG-CMP-ARCH-001 | architecture.md | CMP research pipeline overview |
| DIAG-CMP-ARCH-002 | architecture.md | Phase 0 — CMP foundation |
| DIAG-CMP-ARCH-003 | architecture.md | Phase 1 — composition |
| DIAG-CMP-ARCH-004 | architecture.md | Phase 2 — risk and coherence |
| DIAG-CMP-ARCH-005 | architecture.md | CMP crate dependency graph |
| DIAG-CMP-FLOW-001 | mcp-dispatch.md | CMP tool call flow |
| DIAG-CMP-FLOW-002 | mcp-dispatch.md | CMP caller-mediated seam sequence |
| DIAG-DIA-SWARM-001 | swarm.md | Swarm MCP server architecture (82 tools) |
| DIAG-DIA-SWARM-006 | swarm.md | Swarm server class diagram |
| DIAG-DIA-SWARM-007 | swarm.md | Swarm panel modes (five modes) |
| DIAG-DIA-SWARM-008 | swarm.md | Swarm feedback loops — cybernetic map |
| DIAG-DIA-SWARM-009 | swarm.md | Swarm intelligence PDCA cascade |
| DIAG-DIA-SWARM-010 | swarm.md | Swarm steering loop |
| DIAG-ERD-CREDENTIAL-RESOLUTION-001 | architecture.md | Credential resolution chain ERD |
| DIAG-ES-001 | architecture.md | `hkask-event-store` class diagram |
| DIAG-ONT-001 | architecture.md | Ontology bridge architecture (10 vocabulary modules) |
| DIAG-ONT-002 | architecture.md | Ontology anchor dispatch |
| DIAG-RF-PM | ui-widgets.md | Prediction-markets server class (31 tools) |
| DIAG-SEQ-MCP-TOOL-CALL-001 | mcp-dispatch.md | MCP tool call sequence |
| DIAG-SKILL-CFR | architecture.md | Constraint-forces skills architecture |
| DIAG-STATE-KANBAN-MOVE | kanban.md | Kanban move controller state |
| DIAG-STATE-TASK-STATUS | kanban.md | Task status lifecycle state |
| DIAG-VIZ-CORE | architecture.md | `hkask-viz-core` composition root |
| DIAG-VIZ-GRAPH | ui-widgets.md | Graph widget class |
| DIAG-VIZ-KANBAN | ui-widgets.md | Kanban widget class |
| DIAG-VIZ-PORTFOLIO | ui-widgets.md | Portfolio widget class |
| DIAG-VIZ-SCENARIOS | ui-widgets.md | Scenarios widget class |
| DIAG-VIZ-SWARM | ui-widgets.md | Swarm widget class |

## 3. Diagrams preserved elsewhere

| Former file | New location |
|-------------|--------------|
| erd-memory-store.md | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) |
| flowchart-memory-recall.md | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) |
| sequence-memory-ingest.md | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) |

## 4. Dropped content (2026-08-28 consolidation)

- The CMP **falsification suite** diagrams content (`falsification_log`,
  `h2_duration_test`, `h3_coherence_test`, H1–H5 status log) was dropped from
  DIAG-CMP-ARCH-001/004 and DIAG-CMP-FLOW-001/002 — the underlying functions
  and the `falsification.rs` module were deleted from `hkask-forecast`.
- The ontology bridge's former "Deleted (rip-and-replace)" subgraph was
  dropped — `fibo.rs`, `eso.rs`, `golem.rs`, and `omc.rs` exist again, and
  `sdmx.rs` / `sumo.rs` are new.

## 5. Standards

Diagram format, `DIAGRAM_ALIGNMENT` metadata, and lifecycle are governed by
[`architecture/DOCUMENTATION_STANDARDS.md`](./architecture/DOCUMENTATION_STANDARDS.md)
§2, §4. The `id` is globally unique across the corpus and registered here.
The `verified_against` field must cite a code file, a shipping configuration,
or an external canonical reference — never another prose document.
