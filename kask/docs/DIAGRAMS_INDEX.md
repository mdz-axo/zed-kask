---
title: "hKask Diagram Index — Mermaid Verification Registry"
audience: [architects, developers, agents]
last_updated: 2026-11-26
version: "2.11.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [curation, composition]
---

# hKask Diagram Index — Mermaid Verification Registry

This registry is generated from the surviving `DIAGRAM_ALIGNMENT` metadata under `kask/docs/`. As of 2026-11-26 the corpus contains **77 Mermaid fences, 77 active alignment records, and 77 registry table rows** (`check-diagram-registry-parity.sh` 1:1:1 parity): 17 records in the four consolidated diagram files and 60 inline records.

Retired diagram records are retained in git history rather than as table rows (including the 8 former tombstoned IDs purged from the active table on 2026-11-18 — `DIAG-ARCH-SPEAKER-IDENT-001`, `DIAG-CAP-004` through `006`, `DIAG-RES-CHUNK-001` through `002`, and `DIAG-RES-CNS-001` through `002` — plus earlier retired diagrams removed with `hkask-event-store`, companies, portfolio, spreadsheet, and swarm surfaces). `architecture/DOCUMENTATION_STANDARDS.md` §3's own lifecycle diagram retains its record `DIAG-STD-001`.

## Consolidated files

| File | Alignment records |
| --- | ---: |
| [`diagrams/architecture.md`](./diagrams/architecture.md) | 7 |
| [`diagrams/kanban.md`](./diagrams/kanban.md) | 3 |
| [`diagrams/mcp-dispatch.md`](./diagrams/mcp-dispatch.md) | 2 |
| [`diagrams/ui-widgets.md`](./diagrams/ui-widgets.md) | 5 |
| **Total** | **17** |

## Current metadata registry

Every row below is an actual `(id, location)` metadata pair. Dates and statuses are read from the owning `DIAGRAM_ALIGNMENT` block; no blanket verification date is implied.

| DIAGRAM_ID | Location | Verified date | Status |
| --- | --- | --- | --- |
| `DIAG-ARCH-COMPACTION-001` | [`architecture/compaction-pipeline-spec.md`](./architecture/compaction-pipeline-spec.md) | 2026-11-18 | VERIFIED |
| `DIAG-ARCH-EUREKA-001` | [`research/experimentation-reference-model.md`](./research/experimentation-reference-model.md) | 2026-11-18 | VERIFIED |
| `DIAG-ARCH-LEARNING-LOOP-001` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-11-26 | VERIFIED |
| `DIAG-ARCH-SKILL-MCP-LISP-001` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-11-18 | VERIFIED |
| `DIAG-ARTIFACT-001` | [`architecture/standardized-artifact-storage.md`](./architecture/standardized-artifact-storage.md) | 2026-11-18 | VERIFIED |
| `DIAG-BRIDGE-002` | [`diataxis/kask_bridge/reference.md`](./diataxis/kask_bridge/reference.md) | 2026-11-18 | VERIFIED |
| `DIAG-BRIDGE-003` | [`diataxis/kask_bridge/reference.md`](./diataxis/kask_bridge/reference.md) | 2026-11-18 | VERIFIED |
| `DIAG-BRIDGE-004` | [`diataxis/kask_bridge/reference.md`](./diataxis/kask_bridge/reference.md) | 2026-11-18 | VERIFIED |
| `DIAG-BRIDGE-006` | [`diataxis/kask_bridge/explanation.md`](./diataxis/kask_bridge/explanation.md) | 2026-09-28 | VERIFIED |
| `DIAG-BRIDGE-007` | [`diataxis/kask_bridge/explanation.md`](./diataxis/kask_bridge/explanation.md) | 2026-09-28 | VERIFIED |
| `DIAG-CAP-001` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-11-13 | VERIFIED |
| `DIAG-CAP-002` | [`diagrams/mcp-dispatch.md`](./diagrams/mcp-dispatch.md) | 2026-11-18 | VERIFIED |
| `DIAG-COND-002` | [`diataxis/hkask-condenser/reference.md`](./diataxis/hkask-condenser/reference.md) | 2026-11-18 | VERIFIED |
| `DIAG-COND-003` | [`diataxis/hkask-condenser/reference.md`](./diataxis/hkask-condenser/reference.md) | 2026-11-18 | VERIFIED |
| `DIAG-COND-004` | [`diataxis/hkask-condenser/explanation.md`](./diataxis/hkask-condenser/explanation.md) | 2026-11-18 | VERIFIED |
| `DIAG-ERD-CREDENTIAL-RESOLUTION-001` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-11-18 | VERIFIED |
| `DIAG-FUNCTIONAL-001` | [`architecture/functional-interaction-spec.md`](./architecture/functional-interaction-spec.md) | 2026-11-18 | VERIFIED |
| `DIAG-IC-017` | [`reference/mcp-servers/README.md`](./reference/mcp-servers/README.md) | 2026-11-18 | VERIFIED |
| `DIAG-INF-004` | [`diataxis/hkask-inference/explanation.md`](./diataxis/hkask-inference/explanation.md) | 2026-09-28 | VERIFIED |
| `DIAG-INF-005` | [`diataxis/hkask-inference/explanation.md`](./diataxis/hkask-inference/explanation.md) | 2026-09-28 | VERIFIED |
| `DIAG-INF-PROVIDER` | [`diataxis/hkask-inference/reference.md`](./diataxis/hkask-inference/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-INF-REF` | [`diataxis/hkask-inference/reference.md`](./diataxis/hkask-inference/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-INF-WIRE` | [`diataxis/hkask-inference/reference.md`](./diataxis/hkask-inference/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-MCPSRV-001` | [`diataxis/hkask-mcp-server/tutorial.md`](./diataxis/hkask-mcp-server/tutorial.md) | 2026-10-23 | VERIFIED |
| `DIAG-MCPSRV-010` | [`diataxis/hkask-mcp-server/reference.md`](./diataxis/hkask-mcp-server/reference.md) | 2026-10-23 | VERIFIED |
| `DIAG-MCPSRV-020` | [`diataxis/hkask-mcp-server/reference.md`](./diataxis/hkask-mcp-server/reference.md) | 2026-10-19 | VERIFIED |
| `DIAG-MCPSRV-030` | [`diataxis/hkask-mcp-server/explanation.md`](./diataxis/hkask-mcp-server/explanation.md) | 2026-09-28 | VERIFIED |
| `DIAG-MDS-001` | [`architecture/core/MDS.md`](./architecture/core/MDS.md) | 2026-11-18 | VERIFIED |
| `DIAG-MEDIA-PANEL-001` | [`diataxis/media_panel/reference.md`](./diataxis/media_panel/reference.md) | 2026-11-06 | VERIFIED |
| `DIAG-MEDIA-PANEL-002` | [`diataxis/media_panel/reference.md`](./diataxis/media_panel/reference.md) | 2026-11-06 | VERIFIED |
| `DIAG-MEM-ARCH` | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) | 2026-11-18 | VERIFIED |
| `DIAG-MEM-THERAPY-TOOLS` | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) | 2026-11-18 | VERIFIED |
| `DIAG-MEM-WHO` | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) | 2026-11-18 | VERIFIED |
| `DIAG-ONT-001` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-11-18 | VERIFIED |
| `DIAG-ONT-002` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-11-18 | VERIFIED |
| `DIAG-PL-MEMORY-ERD` | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) | 2026-11-18 | VERIFIED |
| `DIAG-PL-MEMORY-INGEST` | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) | 2026-11-18 | VERIFIED |
| `DIAG-PL-MEMORY-RECALL` | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) | 2026-11-18 | VERIFIED |
| `DIAG-PROMPT-001` | [`architecture/skills-and-composition.md`](./architecture/skills-and-composition.md) | 2026-11-18 | VERIFIED |
| `DIAG-REG-002` | [`diataxis/hkask-regulation/reference.md`](./diataxis/hkask-regulation/reference.md) | 2026-11-18 | VERIFIED |
| `DIAG-REG-003` | [`diataxis/hkask-regulation/reference.md`](./diataxis/hkask-regulation/reference.md) | 2026-10-22 | VERIFIED |
| `DIAG-REG-004` | [`diataxis/hkask-regulation/reference.md`](./diataxis/hkask-regulation/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-REG-005` | [`diataxis/hkask-regulation/explanation.md`](./diataxis/hkask-regulation/explanation.md) | 2026-10-22 | VERIFIED |
| `DIAG-REG-006` | [`diataxis/hkask-regulation/explanation.md`](./diataxis/hkask-regulation/explanation.md) | 2026-11-10 | VERIFIED |
| `DIAG-REG-007` | [`diataxis/hkask-regulation/explanation.md`](./diataxis/hkask-regulation/explanation.md) | 2026-09-28 | VERIFIED |
| `DIAG-RF-003` | [`reference/mcp-servers/README.md`](./reference/mcp-servers/README.md) | 2026-11-18 | VERIFIED |
| `DIAG-RF-005` | [`reference/mcp-servers/scenarios.md`](./reference/mcp-servers/scenarios.md) | 2026-10-06 | VERIFIED |
| `DIAG-RF-006` | [`reference/mcp-servers/media.md`](./reference/mcp-servers/media.md) | 2026-11-18 | VERIFIED |
| `DIAG-RF-007` | [`reference/mcp-servers/media.md`](./reference/mcp-servers/media.md) | 2026-10-14 | VERIFIED |
| `DIAG-RF-008` | [`reference/mcp-servers/media.md`](./reference/mcp-servers/media.md) | 2026-10-14 | VERIFIED |
| `DIAG-RF-PM-PIPELINE-001` | [`diagrams/ui-widgets.md`](./diagrams/ui-widgets.md) | 2026-11-18 | VERIFIED |
| `DIAG-RF-PM` | [`diagrams/ui-widgets.md`](./diagrams/ui-widgets.md) | 2026-11-18 | VERIFIED |
| `DIAG-SEQ-MCP-TOOL-CALL-001` | [`diagrams/mcp-dispatch.md`](./diagrams/mcp-dispatch.md) | 2026-11-18 | VERIFIED |
| `DIAG-SKILL-COMPOSITION-001` | [`architecture/skills-and-composition.md`](./architecture/skills-and-composition.md) | 2026-11-26 | VERIFIED |
| `DIAG-STATE-GOAL-LIFECYCLE` | [`diagrams/kanban.md`](./diagrams/kanban.md) | 2026-11-18 | VERIFIED |
| `DIAG-STATE-KANBAN-MOVE` | [`diagrams/kanban.md`](./diagrams/kanban.md) | 2026-09-28 | VERIFIED |
| `DIAG-STATE-TASK-STATUS` | [`diagrams/kanban.md`](./diagrams/kanban.md) | 2026-11-18 | VERIFIED |
| `DIAG-STD-001` | [`architecture/DOCUMENTATION_STANDARDS.md`](./architecture/DOCUMENTATION_STANDARDS.md) | 2026-11-18 | VERIFIED |
| `DIAG-STOR-002` | [`diataxis/hkask-storage/reference.md`](./diataxis/hkask-storage/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-STOR-003` | [`diataxis/hkask-storage/reference.md`](./diataxis/hkask-storage/reference.md) | 2026-10-13 | VERIFIED |
| `DIAG-STOR-005` | [`diataxis/hkask-storage/explanation.md`](./diataxis/hkask-storage/explanation.md) | 2026-09-28 | VERIFIED |
| `DIAG-STOR-007` | [`diataxis/hkask-storage/reference.md`](./diataxis/hkask-storage/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-STOR-008` | [`diataxis/hkask-storage/reference.md`](./diataxis/hkask-storage/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-TYPES-002` | [`diataxis/hkask-types/reference.md`](./diataxis/hkask-types/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-TYPES-003` | [`diataxis/hkask-types/reference.md`](./diataxis/hkask-types/reference.md) | 2026-10-23 | VERIFIED |
| `DIAG-TYPES-004` | [`diataxis/hkask-types/reference.md`](./diataxis/hkask-types/reference.md) | 2026-11-18 | VERIFIED |
| `DIAG-TYPES-005` | [`diataxis/hkask-types/reference.md`](./diataxis/hkask-types/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-TYPES-006` | [`diataxis/hkask-types/reference.md`](./diataxis/hkask-types/reference.md) | 2026-10-23 | VERIFIED |
| `DIAG-TYPES-008` | [`diataxis/hkask-types/explanation.md`](./diataxis/hkask-types/explanation.md) | 2026-11-18 | VERIFIED |
| `DIAG-TYPES-009` | [`diataxis/hkask-types/explanation.md`](./diataxis/hkask-types/explanation.md) | 2026-09-28 | VERIFIED |
| `DIAG-TYPES-010` | [`diataxis/hkask-types/reference.md`](./diataxis/hkask-types/reference.md) | 2026-11-18 | VERIFIED |
| `DIAG-VIZ-CORE` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-11-18 | VERIFIED |
| `DIAG-VIZ-GRAPH` | [`diagrams/ui-widgets.md`](./diagrams/ui-widgets.md) | 2026-10-21 | VERIFIED |
| `DIAG-VIZ-KANBAN` | [`diagrams/ui-widgets.md`](./diagrams/ui-widgets.md) | 2026-10-28 | VERIFIED |
| `DIAG-VIZ-SCENARIOS` | [`diagrams/ui-widgets.md`](./diagrams/ui-widgets.md) | 2026-10-28 | VERIFIED |
| `DIAG-WALLET-001` | [`research/wallet-cashu-lightning-design.md`](./research/wallet-cashu-lightning-design.md) | 2026-11-26 | VERIFIED |
| `DIAG-WALLET-002` | [`research/wallet-cashu-lightning-design.md`](./research/wallet-cashu-lightning-design.md) | 2026-11-26 | VERIFIED |

Metadata-date distribution: 4 records verified 2026-11-26 (skill inventory/composition + wallet design diagrams), 34 records verified 2026-11-18 during the full-corpus realignment pass, 20 at 2026-09-28, 4 at 2026-10-23, 2 each at 2026-10-14, 2026-10-22, 2026-10-28, and 2026-11-06, and 1 each at 2026-10-06, 2026-10-13, 2026-10-19, 2026-10-20, 2026-10-21, 2026-11-10, and 2026-11-13. All 77 active records carry `status: VERIFIED`.

## Preserved memory diagrams

The former standalone memory diagrams remain inline in [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) under IDs `DIAG-PL-MEMORY-ERD`, `DIAG-PL-MEMORY-INGEST`, and `DIAG-PL-MEMORY-RECALL`.

## Registry rules

- Active IDs are globally unique; each active Mermaid block has exactly one immediately adjacent `DIAGRAM_ALIGNMENT` record.
- A current-state alignment record carries its own `verified_date`, `verified_against`, and `status`; this registry never substitutes a blanket date.
- The example placeholder `DIAG-<AREA>-<NNN>` in the standards is not a registry ID.
- Operator-retained `status: "Proposed"` documents may contain explicitly conceptual future-state Mermaid without implementation alignment until implementation begins.

The governing requirements are in [`architecture/DOCUMENTATION_STANDARDS.md`](./architecture/DOCUMENTATION_STANDARDS.md) §4.
