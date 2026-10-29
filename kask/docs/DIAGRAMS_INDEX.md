---
title: "hKask Diagram Index — Mermaid Verification Registry"
audience: [architects, developers, agents]
last_updated: 2026-10-27
version: "2.10.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [curation, composition]
---

# hKask Diagram Index — Mermaid Verification Registry

This registry is generated from the surviving `DIAGRAM_ALIGNMENT` metadata under `kask/docs/`. As of 2026-10-28 the corpus contains **83 active alignment records representing 83 unique IDs**: 18 records in the four consolidated diagram files and 65 inline records. The 2026-09-28 how-to folds moved eleven inline records from the per-crate how-to documents into their set references' Procedures sections (count unchanged by the folds); the deleted `DIAG-SKILL-CFR` (subject removed with the GSR/CFR skills, commit `5eed0134bf`) reduced the count from 106 to 105.

The corpus contains 83 Mermaid blocks (mechanically counted 2026-10-28: 83 `mermaid` fences, 83 `id: DIAG-` records; the 2026-10-18→22 removal waves deleted four inline diagrams — `DIAG-CAP-003`, `DIAG-MCPSRV-031`, `DIAG-RF-SWARM-001`, `DIAG-SWARM-031` — without same-pass row deletion, which left the parity gate red on main (92 fences vs 95 rows) until this pass reconciled it; this pass also registers `DIAG-ARCH-EUREKA-001`, the eureka-cycle implementation plan's loop diagram; the 2026-10-27 speaker-identity plan pass registers `DIAG-ARCH-SPEAKER-IDENT-001`, the educt speaker-identity plan's §5 suggest-only identification flow; the two `DIAG-RF-004`/`DIAG-RF-004A` rows for the deleted `reference/mcp-servers/companies.md` were removed with the companies capability; the CMP program's eight diagrams and the portfolio widget's one were removed with their programs on 2026-10-15, the spreadsheet capability's two — the LogiSheets plan's architecture block and the widget's class diagram — were removed with the capability the same day, and the agentic-scientist integration proposal's three unregistered illustrative fences were removed with the implemented design on 2026-10-16). `architecture/DOCUMENTATION_STANDARDS.md` §3's own lifecycle diagram is exempt by the self-illustration rule (DOCUMENTATION_STANDARDS §4.2, added 2026-09-30: the standards document's own illustrative diagrams depict the standard's definitions rather than the tree's structure) yet retains its pre-exemption record `DIAG-STD-001` (verified 2026-09-28). The compaction pipeline spec's diagram (landed 2026-09-29) carried no record until this run registered it as `DIAG-ARCH-COMPACTION-001` (2026-10-05) — the 2026-09-30 prose had misattributed the gap to the standards' exempt diagram. On 2026-09-26 the skill learning loop registered as `DIAG-ARCH-LEARNING-LOOP-001` in `diagrams/architecture.md`. The LogiSheets plan's architecture block registered as `DIAG-ARCH-SPREADSHEET-001` on 2026-09-19 (implementation began 2026-09-18, converting the plan to `Active` per `kask/docs/architecture/DOCUMENTATION_STANDARDS.md` §4.2). On 2026-09-30 the three-layer review registered two model diagrams in `research/cybernetic-nervous-system-reference-model.md`: `DIAG-RES-CNS-001` (the abstract tiered feedback structure, verified against the external canonical sources) and `DIAG-RES-CNS-002` (the A/B/C pathway mapping, verified against the layer pathway code sites).

## Consolidated files

| File | Alignment records |
| --- | ---: |
| [`diagrams/architecture.md`](./diagrams/architecture.md) | 8 |
| [`diagrams/kanban.md`](./diagrams/kanban.md) | 3 |
| [`diagrams/mcp-dispatch.md`](./diagrams/mcp-dispatch.md) | 2 |
| [`diagrams/ui-widgets.md`](./diagrams/ui-widgets.md) | 6 |
| **Total** | **16** |

## Current metadata registry

Every row below is an actual `(id, location)` metadata pair. Dates and statuses are read from the owning `DIAGRAM_ALIGNMENT` block; no blanket verification date is implied.

| DIAGRAM_ID | Location | Verified date | Status |
| --- | --- | --- | --- |
| `DIAG-ARCH-LEARNING-LOOP-001` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-09-28 | VERIFIED |
| `DIAG-ARCH-SKILL-MCP-LISP-001` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-10-19 | VERIFIED |
| `DIAG-ARCH-COMPACTION-001` | [`architecture/compaction-pipeline-spec.md`](./architecture/compaction-pipeline-spec.md) | 2026-10-05 | VERIFIED |
| `DIAG-ARCH-EUREKA-001` | [`plans/eureka-cycle-implementation-plan.md`](./plans/eureka-cycle-implementation-plan.md) | 2026-10-23 | VERIFIED |
| `DIAG-ARCH-SPEAKER-IDENT-001` | [`plans/educt-speaker-identity-plan-2026-10-27.md`](./plans/educt-speaker-identity-plan-2026-10-27.md) | 2026-10-27 | VERIFIED |
| `DIAG-ARTIFACT-001` | [`architecture/standardized-artifact-storage.md`](./architecture/standardized-artifact-storage.md) | 2026-09-28 | VERIFIED |
| `DIAG-BRIDGE-002` | [`diataxis/kask_bridge/reference.md`](./diataxis/kask_bridge/reference.md) | 2026-10-19 | VERIFIED |
| `DIAG-BRIDGE-003` | [`diataxis/kask_bridge/reference.md`](./diataxis/kask_bridge/reference.md) | 2026-10-19 | VERIFIED |
| `DIAG-BRIDGE-004` | [`diataxis/kask_bridge/reference.md`](./diataxis/kask_bridge/reference.md) | 2026-10-19 | VERIFIED |
| `DIAG-BRIDGE-006` | [`diataxis/kask_bridge/explanation.md`](./diataxis/kask_bridge/explanation.md) | 2026-09-28 | VERIFIED |
| `DIAG-BRIDGE-007` | [`diataxis/kask_bridge/explanation.md`](./diataxis/kask_bridge/explanation.md) | 2026-09-28 | VERIFIED |
| `DIAG-CAP-001` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-09-28 | VERIFIED |
| `DIAG-CAP-002` | [`diagrams/mcp-dispatch.md`](./diagrams/mcp-dispatch.md) | 2026-09-28 | VERIFIED |

| `DIAG-CAP-004` | [`diataxis/hkask-tool-port/reference.md`](./diataxis/hkask-tool-port/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-CAP-005` | [`diataxis/hkask-tool-port/reference.md`](./diataxis/hkask-tool-port/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-CAP-006` | [`diataxis/hkask-tool-port/reference.md`](./diataxis/hkask-tool-port/reference.md) | 2026-09-28 | VERIFIED |

| `DIAG-COND-002` | [`diataxis/hkask-condenser/reference.md`](./diataxis/hkask-condenser/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-COND-003` | [`diataxis/hkask-condenser/reference.md`](./diataxis/hkask-condenser/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-COND-004` | [`diataxis/hkask-condenser/explanation.md`](./diataxis/hkask-condenser/explanation.md) | 2026-09-28 | VERIFIED |
| `DIAG-ERD-CREDENTIAL-RESOLUTION-001` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-09-28 | VERIFIED |
| `DIAG-ES-001` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-09-28 | VERIFIED |
| `DIAG-FUNCTIONAL-001` | [`architecture/functional-interaction-spec.md`](./architecture/functional-interaction-spec.md) | 2026-09-28 | VERIFIED |
| `DIAG-IC-017` | [`reference/mcp-servers/README.md`](./reference/mcp-servers/README.md) | 2026-09-28 | VERIFIED |
| `DIAG-INF-004` | [`diataxis/hkask-inference/explanation.md`](./diataxis/hkask-inference/explanation.md) | 2026-09-28 | VERIFIED |
| `DIAG-INF-005` | [`diataxis/hkask-inference/explanation.md`](./diataxis/hkask-inference/explanation.md) | 2026-09-28 | VERIFIED |
| `DIAG-INF-PROVIDER` | [`diataxis/hkask-inference/reference.md`](./diataxis/hkask-inference/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-INF-REF` | [`diataxis/hkask-inference/reference.md`](./diataxis/hkask-inference/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-INF-WIRE` | [`diataxis/hkask-inference/reference.md`](./diataxis/hkask-inference/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-MCPSRV-001` | [`diataxis/hkask-mcp-server/tutorial.md`](./diataxis/hkask-mcp-server/tutorial.md) | 2026-09-28 | VERIFIED |
| `DIAG-MCPSRV-010` | [`diataxis/hkask-mcp-server/reference.md`](./diataxis/hkask-mcp-server/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-MCPSRV-020` | [`diataxis/hkask-mcp-server/reference.md`](./diataxis/hkask-mcp-server/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-MCPSRV-030` | [`diataxis/hkask-mcp-server/explanation.md`](./diataxis/hkask-mcp-server/explanation.md) | 2026-09-28 | VERIFIED |

| `DIAG-MDS-001` | [`architecture/core/MDS.md`](./architecture/core/MDS.md) | 2026-10-19 | VERIFIED |
| `DIAG-MEDIA-PANEL-001` | [`diataxis/media_panel/reference.md`](./diataxis/media_panel/reference.md) | 2026-10-26 | VERIFIED |
| `DIAG-MEDIA-PANEL-002` | [`diataxis/media_panel/reference.md`](./diataxis/media_panel/reference.md) | 2026-10-26 | VERIFIED |
| `DIAG-MEM-ARCH` | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) | 2026-09-28 | VERIFIED |
| `DIAG-MEM-THERAPY-TOOLS` | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) | 2026-09-28 | VERIFIED |
| `DIAG-MEM-WHO` | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) | 2026-09-28 | VERIFIED |
| `DIAG-ONT-001` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-09-28 | VERIFIED |
| `DIAG-ONT-002` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-09-28 | VERIFIED |
| `DIAG-PL-MEMORY-ERD` | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) | 2026-09-28 | VERIFIED |
| `DIAG-PL-MEMORY-INGEST` | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) | 2026-09-28 | VERIFIED |
| `DIAG-PL-MEMORY-RECALL` | [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) | 2026-09-28 | VERIFIED |
| `DIAG-PROMPT-001` | [`architecture/skills-and-composition.md`](./architecture/skills-and-composition.md) | 2026-09-28 | VERIFIED |
| `DIAG-REG-002` | [`diataxis/hkask-regulation/reference.md`](./diataxis/hkask-regulation/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-REG-003` | [`diataxis/hkask-regulation/reference.md`](./diataxis/hkask-regulation/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-REG-004` | [`diataxis/hkask-regulation/reference.md`](./diataxis/hkask-regulation/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-REG-005` | [`diataxis/hkask-regulation/explanation.md`](./diataxis/hkask-regulation/explanation.md) | 2026-09-28 | VERIFIED |
| `DIAG-REG-006` | [`diataxis/hkask-regulation/explanation.md`](./diataxis/hkask-regulation/explanation.md) | 2026-09-28 | VERIFIED |
| `DIAG-REG-007` | [`diataxis/hkask-regulation/explanation.md`](./diataxis/hkask-regulation/explanation.md) | 2026-09-28 | VERIFIED |
| `DIAG-RES-CHUNK-001` | [`research/chunking-for-rag-research.md`](./research/chunking-for-rag-research.md) | 2026-09-16 | VERIFIED |
| `DIAG-RES-CHUNK-002` | [`research/chunking-for-rag-research.md`](./research/chunking-for-rag-research.md) | 2026-09-28 | VERIFIED |

| `DIAG-RES-CNS-001` | [`research/cybernetic-nervous-system-reference-model.md`](./research/cybernetic-nervous-system-reference-model.md) | 2026-09-30 | VERIFIED |
| `DIAG-RES-CNS-002` | [`research/cybernetic-nervous-system-reference-model.md`](./research/cybernetic-nervous-system-reference-model.md) | 2026-09-30 | VERIFIED |
| `DIAG-RF-003` | [`reference/mcp-servers/README.md`](./reference/mcp-servers/README.md) | 2026-09-28 | VERIFIED |
| `DIAG-RF-005` | [`reference/mcp-servers/scenarios.md`](./reference/mcp-servers/scenarios.md) | 2026-09-28 | VERIFIED |
| `DIAG-RF-006` | [`reference/mcp-servers/media.md`](./reference/mcp-servers/media.md) | 2026-09-28 | VERIFIED |
| `DIAG-RF-007` | [`reference/mcp-servers/media.md`](./reference/mcp-servers/media.md) | 2026-10-13 | VERIFIED |
| `DIAG-RF-008` | [`reference/mcp-servers/media.md`](./reference/mcp-servers/media.md) | 2026-10-13 | VERIFIED |
| `DIAG-RF-PM` | [`diagrams/ui-widgets.md`](./diagrams/ui-widgets.md) | 2026-09-28 | VERIFIED |
| `DIAG-RF-PM-PIPELINE-001` | [`diagrams/ui-widgets.md`](./diagrams/ui-widgets.md) | 2026-09-28 | VERIFIED |

| `DIAG-SEQ-MCP-TOOL-CALL-001` | [`diagrams/mcp-dispatch.md`](./diagrams/mcp-dispatch.md) | 2026-09-28 | VERIFIED |

| `DIAG-STATE-GOAL-LIFECYCLE` | [`diagrams/kanban.md`](./diagrams/kanban.md) | 2026-09-28 | VERIFIED |
| `DIAG-STATE-KANBAN-MOVE` | [`diagrams/kanban.md`](./diagrams/kanban.md) | 2026-09-28 | VERIFIED |
| `DIAG-STATE-TASK-STATUS` | [`diagrams/kanban.md`](./diagrams/kanban.md) | 2026-09-28 | VERIFIED |
| `DIAG-STD-001` | [`architecture/DOCUMENTATION_STANDARDS.md`](./architecture/DOCUMENTATION_STANDARDS.md) | 2026-09-28 | VERIFIED |
| `DIAG-STOR-002` | [`diataxis/hkask-storage/reference.md`](./diataxis/hkask-storage/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-STOR-003` | [`diataxis/hkask-storage/reference.md`](./diataxis/hkask-storage/reference.md) | 2026-10-13 | VERIFIED |
| `DIAG-STOR-005` | [`diataxis/hkask-storage/explanation.md`](./diataxis/hkask-storage/explanation.md) | 2026-09-28 | VERIFIED |
| `DIAG-STOR-007` | [`diataxis/hkask-storage/reference.md`](./diataxis/hkask-storage/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-STOR-008` | [`diataxis/hkask-storage/reference.md`](./diataxis/hkask-storage/reference.md) | 2026-09-28 | VERIFIED |

| `DIAG-TYPES-002` | [`diataxis/hkask-types/reference.md`](./diataxis/hkask-types/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-TYPES-003` | [`diataxis/hkask-types/reference.md`](./diataxis/hkask-types/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-TYPES-004` | [`diataxis/hkask-types/reference.md`](./diataxis/hkask-types/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-TYPES-005` | [`diataxis/hkask-types/reference.md`](./diataxis/hkask-types/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-TYPES-006` | [`diataxis/hkask-types/reference.md`](./diataxis/hkask-types/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-TYPES-008` | [`diataxis/hkask-types/explanation.md`](./diataxis/hkask-types/explanation.md) | 2026-09-28 | VERIFIED |
| `DIAG-TYPES-009` | [`diataxis/hkask-types/explanation.md`](./diataxis/hkask-types/explanation.md) | 2026-09-28 | VERIFIED |
| `DIAG-TYPES-010` | [`diataxis/hkask-types/reference.md`](./diataxis/hkask-types/reference.md) | 2026-09-28 | VERIFIED |
| `DIAG-VIZ-CORE` | [`diagrams/architecture.md`](./diagrams/architecture.md) | 2026-09-28 | VERIFIED |
| `DIAG-VIZ-GRAPH` | [`diagrams/ui-widgets.md`](./diagrams/ui-widgets.md) | 2026-09-28 | VERIFIED |
| `DIAG-VIZ-KANBAN` | [`diagrams/ui-widgets.md`](./diagrams/ui-widgets.md) | 2026-09-28 | VERIFIED |

| `DIAG-VIZ-SCENARIOS` | [`diagrams/ui-widgets.md`](./diagrams/ui-widgets.md) | 2026-09-28 | VERIFIED |

Metadata-date distribution: 81 records at 2026-09-28, five at 2026-10-19 (the realignment pass re-verifying the companies-wave-touched records: `DIAG-ARCH-SKILL-MCP-LISP-001`, `DIAG-BRIDGE-002`, `DIAG-BRIDGE-003`, `DIAG-BRIDGE-004`, `DIAG-MDS-001`), three at 2026-10-13, two at 2026-10-26 (the media_panel diataxis re-anchor pass re-verifying `DIAG-MEDIA-PANEL-001` and `DIAG-MEDIA-PANEL-002` against the five-tab viewer), two at 2026-09-30 (the cybernetic-nervous-system model diagrams), one at 2026-10-05, and one externally-anchored record at 2026-09-16 (`DIAG-RES-CHUNK-001`, verified against arXiv/Anthropic engineering sources rather than tree files). All active records are `VERIFIED` (one tombstoned ID mention is excluded from the active count).

## Retired/tombstone IDs (excluded from active registry validation)

The 2026-09-16 tutorial folds removed these registry IDs with their owning documents: `DIAG-COND-001`, `DIAG-INF-001`, `DIAG-REG-001`, `DIAG-STOR-001`, `DIAG-TYPES-001`, `DIAG-BRIDGE-001`, `DIAG-SWARM-001`, `DIAG-SWARM-002`, and `DIAG-SWARM-003`. The deleted `hkask-tool-port/tutorial.md` carried no registered diagram ID. Their document successors are recorded in the lifecycle ledger at [`README.md`](./README.md).

## Preserved memory diagrams

The former standalone memory diagrams remain inline in [`architecture/memory-system-specification.md`](./architecture/memory-system-specification.md) under IDs `DIAG-PL-MEMORY-ERD`, `DIAG-PL-MEMORY-INGEST`, and `DIAG-PL-MEMORY-RECALL`.

## Registry rules

- Active IDs are globally unique; each active Mermaid block has exactly one immediately adjacent `DIAGRAM_ALIGNMENT` record.
- A current-state alignment record carries its own `verified_date`, `verified_against`, and `status`; this registry never substitutes a blanket date.
- The example placeholder `DIAG-<AREA>-<NNN>` in the standards is not a registry ID.
- Operator-retained `status: "Proposed"` documents may contain explicitly conceptual future-state Mermaid without implementation alignment until implementation begins.

The governing requirements are in [`architecture/DOCUMENTATION_STANDARDS.md`](./architecture/DOCUMENTATION_STANDARDS.md) §4.
