---
title: "hKask Diagram Index — Mermaid Verification Registry"
audience: [architects, developers, agents]
last_updated: 2026-09-15
version: "2.0.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [curation, composition]
---

# hKask Diagram Index — Mermaid Verification Registry

This registry is generated from the surviving `DIAGRAM_ALIGNMENT` metadata under `kask/docs/`. On 2026-09-15 the corpus contains **98 alignment records representing 97 unique IDs**: 31 records in the five consolidated diagram files and 67 inline records. `DIAG-CAP-002` is the sole repeated ID and intentionally maps to both the consolidated MCP dispatch diagram and the tool-port reference.

The corpus contains 100 Mermaid blocks: 99 current-state blocks and one explicitly conceptual block in the operator-retained Proposed LogiSheets plan. `DIAG-RF-PM` covers two contiguous current-state views of the same prediction-markets subject in `kask/docs/diagrams/ui-widgets.md:390-550`; the remaining current-state blocks each have one adjacent alignment record. The Proposed block is exempt until implementation under `kask/docs/architecture/DOCUMENTATION_STANDARDS.md` §4.2.

## Consolidated files

| File | Alignment records |
| --- | ---: |
| [`diagrams/architecture.md`](./diagrams/architecture.md) | 13 |
| [`diagrams/kanban.md`](./diagrams/kanban.md) | 2 |
| [`diagrams/mcp-dispatch.md`](./diagrams/mcp-dispatch.md) | 4 |
| [`diagrams/swarm.md`](./diagrams/swarm.md) | 6 |
| [`diagrams/ui-widgets.md`](./diagrams/ui-widgets.md) | 6 |
| **Total** | **31** |

## Current metadata registry

Every row below is an actual `(id, location)` metadata pair. Dates and statuses are read from the owning `DIAGRAM_ALIGNMENT` block; no blanket verification date is implied.

| DIAGRAM_ID | Location | Verified date | Status |
| --- | --- | --- | --- |
| `DIAG-ARCH-SKILL-MCP-LISP-001` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-08-28 | VERIFIED |
| `DIAG-ARTIFACT-001` | [`architecture/standardized-artifact-storage.md`](./architecture/standardized-artifact-storage.md) | 2026-09-15 | VERIFIED |
| `DIAG-BRIDGE-002` | [`diataxis/kask_bridge/how-to.md`](./diataxis/kask_bridge/how-to.md) | 2026-09-15 | VERIFIED |
| `DIAG-BRIDGE-003` | [`diataxis/kask_bridge/reference.md`](./diataxis/kask_bridge/reference.md) | 2026-09-15 | VERIFIED |
| `DIAG-BRIDGE-004` | [`diataxis/kask_bridge/reference.md`](./diataxis/kask_bridge/reference.md) | 2026-09-15 | VERIFIED |
| `DIAG-BRIDGE-006` | [`diataxis/kask_bridge/explanation.md`](./diataxis/kask_bridge/explanation.md) | 2026-09-15 | VERIFIED |
| `DIAG-BRIDGE-007` | [`diataxis/kask_bridge/explanation.md`](./diataxis/kask_bridge/explanation.md) | 2026-09-15 | VERIFIED |
| `DIAG-CAP-001` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-08-28 | VERIFIED |
| `DIAG-CAP-002` | [`diagrams/mcp-dispatch.md`](./diagrams/mcp-dispatch.md) | 2026-08-28 | VERIFIED |
| `DIAG-CAP-002` | [`diataxis/hkask-tool-port/reference.md`](./diataxis/hkask-tool-port/reference.md) | 2026-08-28 | VERIFIED |
| `DIAG-CAP-003` | [`diataxis/hkask-tool-port/reference.md`](./diataxis/hkask-tool-port/reference.md) | 2026-08-28 | VERIFIED |
| `DIAG-CAP-004` | [`diataxis/hkask-tool-port/explanation.md`](./diataxis/hkask-tool-port/explanation.md) | 2026-08-28 | VERIFIED |
| `DIAG-CAP-005` | [`diataxis/hkask-tool-port/explanation.md`](./diataxis/hkask-tool-port/explanation.md) | 2026-08-28 | VERIFIED |
| `DIAG-CMP-ARCH-001` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-08-28 | VERIFIED |
| `DIAG-CMP-ARCH-002` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-08-28 | VERIFIED |
| `DIAG-CMP-ARCH-003` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-08-28 | VERIFIED |
| `DIAG-CMP-ARCH-004` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-08-28 | VERIFIED |
| `DIAG-CMP-ARCH-005` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-08-28 | VERIFIED |
| `DIAG-CMP-FLOW-001` | [`diagrams/mcp-dispatch.md`](./diagrams/mcp-dispatch.md) | 2026-08-28 | VERIFIED |
| `DIAG-CMP-FLOW-002` | [`diagrams/mcp-dispatch.md`](./diagrams/mcp-dispatch.md) | 2026-08-28 | VERIFIED |
| `DIAG-COND-002` | [`diataxis/hkask-condenser/how-to.md`](./diataxis/hkask-condenser/how-to.md) | 2026-09-15 | VERIFIED |
| `DIAG-COND-003` | [`diataxis/hkask-condenser/reference.md`](./diataxis/hkask-condenser/reference.md) | 2026-09-15 | VERIFIED |
| `DIAG-COND-004` | [`diataxis/hkask-condenser/explanation.md`](./diataxis/hkask-condenser/explanation.md) | 2026-09-15 | VERIFIED |
| `DIAG-DIA-SWARM-001` | [`diagrams/swarm.md`](./diagrams/swarm.md) | 2026-09-09 | VERIFIED |
| `DIAG-DIA-SWARM-006` | [`diagrams/swarm.md`](./diagrams/swarm.md) | 2026-09-09 | VERIFIED |
| `DIAG-DIA-SWARM-007` | [`diagrams/swarm.md`](./diagrams/swarm.md) | 2026-08-28 | VERIFIED |
| `DIAG-DIA-SWARM-008` | [`diagrams/swarm.md`](./diagrams/swarm.md) | 2026-08-28 | VERIFIED |
| `DIAG-DIA-SWARM-009` | [`diagrams/swarm.md`](./diagrams/swarm.md) | 2026-08-28 | VERIFIED |
| `DIAG-DIA-SWARM-010` | [`diagrams/swarm.md`](./diagrams/swarm.md) | 2026-08-28 | VERIFIED |
| `DIAG-ERD-CREDENTIAL-RESOLUTION-001` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-08-28 | VERIFIED |
| `DIAG-ES-001` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-08-28 | VERIFIED |
| `DIAG-FUNCTIONAL-001` | [`architecture/functional-interaction-spec.md`](./architecture/functional-interaction-spec.md) | 2026-09-15 | VERIFIED |
| `DIAG-IC-017` | [`reference/mcp-servers/README.md`](./reference/mcp-servers/README.md) | 2026-09-15 | VERIFIED |
| `DIAG-INF-004` | [`diataxis/hkask-inference/explanation.md`](./diataxis/hkask-inference/explanation.md) | 2026-09-15 | VERIFIED |
| `DIAG-INF-005` | [`diataxis/hkask-inference/explanation.md`](./diataxis/hkask-inference/explanation.md) | 2026-09-15 | VERIFIED |
| `DIAG-INF-PROVIDER` | [`diataxis/hkask-inference/how-to.md`](./diataxis/hkask-inference/how-to.md) | 2026-09-15 | VERIFIED |
| `DIAG-INF-REF` | [`diataxis/hkask-inference/reference.md`](./diataxis/hkask-inference/reference.md) | 2026-09-15 | VERIFIED |
| `DIAG-INF-WIRE` | [`diataxis/hkask-inference/how-to.md`](./diataxis/hkask-inference/how-to.md) | 2026-09-15 | VERIFIED |
| `DIAG-MCPSRV-001` | [`diataxis/hkask-mcp-server/tutorial.md`](./diataxis/hkask-mcp-server/tutorial.md) | 2026-09-15 | VERIFIED |
| `DIAG-MCPSRV-010` | [`diataxis/hkask-mcp-server/how-to.md`](./diataxis/hkask-mcp-server/how-to.md) | 2026-09-15 | VERIFIED |
| `DIAG-MCPSRV-020` | [`diataxis/hkask-mcp-server/reference.md`](./diataxis/hkask-mcp-server/reference.md) | 2026-09-15 | VERIFIED |
| `DIAG-MCPSRV-030` | [`diataxis/hkask-mcp-server/explanation.md`](./diataxis/hkask-mcp-server/explanation.md) | 2026-09-15 | VERIFIED |
| `DIAG-MCPSRV-031` | [`diataxis/hkask-mcp-server/explanation.md`](./diataxis/hkask-mcp-server/explanation.md) | 2026-09-15 | VERIFIED |
| `DIAG-MDS-001` | [`architecture/core/MDS.md`](./architecture/core/MDS.md) | 2026-09-15 | VERIFIED |
| `DIAG-MEDIA-PANEL-001` | [`diataxis/media_panel/reference.md`](./diataxis/media_panel/reference.md) | 2026-09-15 | VERIFIED |
| `DIAG-MEDIA-PANEL-002` | [`diataxis/media_panel/reference.md`](./diataxis/media_panel/reference.md) | 2026-09-15 | VERIFIED |
| `DIAG-MEM-ARCH` | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) | 2026-09-04 | VERIFIED |
| `DIAG-MEM-THERAPY-TOOLS` | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) | 2026-09-04 | VERIFIED |
| `DIAG-MEM-WHO` | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) | 2026-08-28 | VERIFIED |
| `DIAG-ONT-001` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-08-28 | VERIFIED |
| `DIAG-ONT-002` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-08-28 | VERIFIED |
| `DIAG-PL-MEMORY-ERD` | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) | 2026-08-28 | VERIFIED |
| `DIAG-PL-MEMORY-INGEST` | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) | 2026-09-04 | VERIFIED |
| `DIAG-PL-MEMORY-RECALL` | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) | 2026-09-04 | VERIFIED |
| `DIAG-PROMPT-001` | [`architecture/skills-and-composition.md`](./architecture/skills-and-composition.md) | 2026-09-15 | VERIFIED |
| `DIAG-REG-002` | [`diataxis/hkask-regulation/how-to.md`](./diataxis/hkask-regulation/how-to.md) | 2026-09-15 | VERIFIED |
| `DIAG-REG-003` | [`diataxis/hkask-regulation/reference.md`](./diataxis/hkask-regulation/reference.md) | 2026-09-15 | VERIFIED |
| `DIAG-REG-004` | [`diataxis/hkask-regulation/reference.md`](./diataxis/hkask-regulation/reference.md) | 2026-09-15 | VERIFIED |
| `DIAG-REG-005` | [`diataxis/hkask-regulation/explanation.md`](./diataxis/hkask-regulation/explanation.md) | 2026-09-15 | VERIFIED |
| `DIAG-REG-006` | [`diataxis/hkask-regulation/explanation.md`](./diataxis/hkask-regulation/explanation.md) | 2026-09-15 | VERIFIED |
| `DIAG-REG-007` | [`diataxis/hkask-regulation/explanation.md`](./diataxis/hkask-regulation/explanation.md) | 2026-09-15 | VERIFIED |
| `DIAG-RF-003` | [`reference/mcp-servers/README.md`](./reference/mcp-servers/README.md) | 2026-09-15 | VERIFIED |
| `DIAG-RF-004` | [`reference/mcp-servers/companies.md`](./reference/mcp-servers/companies.md) | 2026-09-15 | VERIFIED |
| `DIAG-RF-004A` | [`reference/mcp-servers/companies.md`](./reference/mcp-servers/companies.md) | 2026-09-15 | VERIFIED |
| `DIAG-RF-005` | [`reference/mcp-servers/scenarios.md`](./reference/mcp-servers/scenarios.md) | 2026-09-15 | VERIFIED |
| `DIAG-RF-006` | [`reference/mcp-servers/media.md`](./reference/mcp-servers/media.md) | 2026-09-15 | VERIFIED |
| `DIAG-RF-PM` | [`diagrams/ui-widgets.md`](./diagrams/ui-widgets.md) | 2026-08-28 | VERIFIED |
| `DIAG-RF-SWARM-001` | [`reference/mcp-servers/swarm.md`](./reference/mcp-servers/swarm.md) | 2026-09-15 | VERIFIED |
| `DIAG-SEQ-MCP-TOOL-CALL-001` | [`diagrams/mcp-dispatch.md`](./diagrams/mcp-dispatch.md) | 2026-08-30 | VERIFIED |
| `DIAG-SKILL-CFR` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-08-28 | VERIFIED |
| `DIAG-STATE-KANBAN-MOVE` | [`diagrams/kanban.md`](./diagrams/kanban.md) | 2026-08-28 | VERIFIED |
| `DIAG-STATE-TASK-STATUS` | [`diagrams/kanban.md`](./diagrams/kanban.md) | 2026-08-28 | VERIFIED |
| `DIAG-STD-001` | [`architecture/DOCUMENTATION_STANDARDS.md`](./architecture/DOCUMENTATION_STANDARDS.md) | 2026-09-15 | VERIFIED |
| `DIAG-STOR-002` | [`diataxis/hkask-storage/how-to.md`](./diataxis/hkask-storage/how-to.md) | 2026-09-15 | VERIFIED |
| `DIAG-STOR-003` | [`diataxis/hkask-storage/reference.md`](./diataxis/hkask-storage/reference.md) | 2026-09-15 | VERIFIED |
| `DIAG-STOR-004` | [`diataxis/hkask-storage/reference.md`](./diataxis/hkask-storage/reference.md) | 2026-09-15 | VERIFIED |
| `DIAG-STOR-005` | [`diataxis/hkask-storage/explanation.md`](./diataxis/hkask-storage/explanation.md) | 2026-09-15 | VERIFIED |
| `DIAG-STOR-006` | [`diataxis/hkask-storage/explanation.md`](./diataxis/hkask-storage/explanation.md) | 2026-09-15 | VERIFIED |
| `DIAG-STOR-007` | [`diataxis/hkask-storage/how-to.md`](./diataxis/hkask-storage/how-to.md) | 2026-09-15 | VERIFIED |
| `DIAG-STOR-008` | [`diataxis/hkask-storage/reference.md`](./diataxis/hkask-storage/reference.md) | 2026-09-15 | VERIFIED |
| `DIAG-SWARM-010` | [`diataxis/swarm_system/how-to.md`](./diataxis/swarm_system/how-to.md) | 2026-09-15 | VERIFIED |
| `DIAG-SWARM-020` | [`diataxis/swarm_system/reference.md`](./diataxis/swarm_system/reference.md) | 2026-09-15 | VERIFIED |
| `DIAG-SWARM-030` | [`diataxis/swarm_system/explanation.md`](./diataxis/swarm_system/explanation.md) | 2026-09-15 | VERIFIED |
| `DIAG-SWARM-031` | [`diataxis/swarm_system/explanation.md`](./diataxis/swarm_system/explanation.md) | 2026-09-15 | VERIFIED |
| `DIAG-TYPES-002` | [`diataxis/hkask-types/how-to.md`](./diataxis/hkask-types/how-to.md) | 2026-09-15 | VERIFIED |
| `DIAG-TYPES-003` | [`diataxis/hkask-types/how-to.md`](./diataxis/hkask-types/how-to.md) | 2026-09-15 | VERIFIED |
| `DIAG-TYPES-004` | [`diataxis/hkask-types/reference.md`](./diataxis/hkask-types/reference.md) | 2026-09-15 | VERIFIED |
| `DIAG-TYPES-005` | [`diataxis/hkask-types/reference.md`](./diataxis/hkask-types/reference.md) | 2026-09-15 | VERIFIED |
| `DIAG-TYPES-006` | [`diataxis/hkask-types/reference.md`](./diataxis/hkask-types/reference.md) | 2026-09-15 | VERIFIED |
| `DIAG-TYPES-008` | [`diataxis/hkask-types/explanation.md`](./diataxis/hkask-types/explanation.md) | 2026-09-15 | VERIFIED |
| `DIAG-TYPES-009` | [`diataxis/hkask-types/explanation.md`](./diataxis/hkask-types/explanation.md) | 2026-09-15 | VERIFIED |
| `DIAG-TYPES-010` | [`diataxis/hkask-types/reference.md`](./diataxis/hkask-types/reference.md) | 2026-09-15 | VERIFIED |
| `DIAG-VIZ-CORE` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-08-28 | VERIFIED |
| `DIAG-VIZ-GRAPH` | [`diagrams/ui-widgets.md`](./diagrams/ui-widgets.md) | 2026-08-28 | VERIFIED |
| `DIAG-VIZ-KANBAN` | [`diagrams/ui-widgets.md`](./diagrams/ui-widgets.md) | 2026-08-28 | VERIFIED |
| `DIAG-VIZ-PORTFOLIO` | [`diagrams/ui-widgets.md`](./diagrams/ui-widgets.md) | 2026-08-28 | VERIFIED |
| `DIAG-VIZ-SCENARIOS` | [`diagrams/ui-widgets.md`](./diagrams/ui-widgets.md) | 2026-08-28 | VERIFIED |
| `DIAG-VIZ-SWARM` | [`diagrams/ui-widgets.md`](./diagrams/ui-widgets.md) | 2026-08-28 | VERIFIED |

Metadata-date distribution: 34 records at 2026-08-28, one at 2026-08-30, four at 2026-09-04, two at 2026-09-09, and 57 at 2026-09-15. All 98 surviving records are `VERIFIED`.

## Deleted tutorial IDs

The 2026-09-15 tutorial folds removed these registry IDs with their owning documents: `DIAG-COND-001`, `DIAG-INF-001`, `DIAG-REG-001`, `DIAG-STOR-001`, `DIAG-TYPES-001`, `DIAG-BRIDGE-001`, `DIAG-SWARM-001`, `DIAG-SWARM-002`, and `DIAG-SWARM-003`. The deleted `hkask-tool-port/tutorial.md` carried no registered diagram ID. Their document successors are recorded in the lifecycle ledger at [`README.md`](./README.md).

## Preserved memory diagrams

The former standalone memory diagrams remain inline in [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) under IDs `DIAG-PL-MEMORY-ERD`, `DIAG-PL-MEMORY-INGEST`, and `DIAG-PL-MEMORY-RECALL`.

## Registry rules

- IDs are globally unique unless one logical diagram is deliberately presented at multiple current locations; every such location is listed.
- A current-state alignment record carries its own `verified_date`, `verified_against`, and `status`; this registry never substitutes a blanket date.
- The example placeholder `DIAG-<AREA>-<NNN>` in the standards is not a registry ID.
- Operator-retained `status: "Proposed"` documents may contain explicitly conceptual future-state Mermaid without implementation alignment until implementation begins.

The governing requirements are in [`architecture/DOCUMENTATION_STANDARDS.md`](./architecture/DOCUMENTATION_STANDARDS.md) §4.
