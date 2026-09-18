---
title: "zed-kask Documentation"
audience: [developers, architects, agents, operators]
last_updated: 2026-09-18
version: "2.3.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle, curation]
---

# zed-kask Documentation

> **zed-kask** is a minimal-divergence fork of the [Zed editor](https://zed.dev) with the hKask agent platform compiled in-process. The agent runtime, skills, Regulation nervous system, and sovereign memory run inside the editor as native surfaces; 12 managed MCP servers are launched as child processes over stdio by Zed's `context_server` host (`kask/crates/kask_bridge/src/mcp_servers.rs:55-541`).

**Canonical reference:** [`architecture/zed-host-architecture-plan.md`](architecture/zed-host-architecture-plan.md) describes the composition root and current crate inventory. The authoritative integration surface is [`DIVERGENCE.md`](../../DIVERGENCE.md), whose current range is D1–D66 (`DIVERGENCE.md:125-196`; retired numbers are retained and never reused).

**Per-crate docs:** [`diataxis/INDEX.md`](diataxis/INDEX.md) lists 28 retained artifacts across 10 cross-cutting sets. Eight tutorials were folded on 2026-09-16; `diataxis/hkask-mcp-server/tutorial.md` is the one retained tutorial (`kask/docs/diataxis/INDEX.md:13-17`).

**Corpus size:** 68 files under `kask/docs/` on 2026-09-18 (measured with `find kask/docs -type f`), within the fewer-than-70 cap.

## Repair and improvement plans

| Document | Status and purpose |
| --- | --- |
| [`hKask core and MCP repair plan`](plans/hkask-core-mcp-repair-improvement-plan.md) | Active, operator-authorized repair plan; no backward-compatibility requirements. §9 separates verified training/gallery/packaging slices from unfinished authority, cancellation, containment, attribution and recovery work. |
| [`Gödel-machine gap closure plan`](plans/goedel-gap-closure-plan.md) | Active, operator-chartered (goal `5e12d79e`); closes Track B gaps (falsifiable outcome claims before self-changes, mandatory empirical acceptance gate, axiom layer in the constraint registry) and scoped Track A (cfg-gated Kani proof pilot on the training math gates) from the 2026-09-18 Gödel-machine research report. |

## Architecture

| Document | Description |
| --- | --- |
| [`zed-host-architecture-plan.md`](architecture/zed-host-architecture-plan.md) | **Canonical architecture** — D1–D66 integration authority, composition root, crate inventory, deletion history. |
| [`standardized-artifact-storage.md`](architecture/standardized-artifact-storage.md) | **D28** — canonical path layout for persistent Kask artifacts. |
| [`memory-system-specification.md`](architecture/memory-system-specification.md) | **Memory system specification** — schema, ingestion, recall, consolidation, decay, hygiene, sovereignty, and embedded diagrams. |
| [`skills-and-composition.md`](architecture/skills-and-composition.md) | **Agent system** — prompt surfaces, skill body injection, composition principles, and testing. |
| [`functional-interaction-spec.md`](architecture/functional-interaction-spec.md) | **Division of Responsibilities** — operator/product-manager and agent/program-manager working agreement. |
| [`core/PRINCIPLES.md`](architecture/core/PRINCIPLES.md) | Architecture principles P1–P12. |
| [`core/magna-carta.md`](architecture/core/magna-carta.md) | The Magna Carta — four sovereignty principles. |
| [`core/MDS.md`](architecture/core/MDS.md) | Minimal Domain Specification — five-category taxonomy, 18 library/composition crates, and 12 MCP servers. |
| [`DOCUMENTATION_STANDARDS.md`](architecture/DOCUMENTATION_STANDARDS.md) | Metadata, lifecycle, Mermaid alignment, citation, and writing standards, including operator-retained Proposed plans. |

## Reference

| Document | Description |
| --- | --- |
| [`reference/regulation-spans.md`](reference/regulation-spans.md) | Regulation tracing, persisted records, and actual MCP outcome paths. |
| [`reference/mcp-servers/README.md`](reference/mcp-servers/README.md) | MCP server registry — 12 built-in servers and the fleet tool surface. |
| [`reference/mcp-servers/companies.md`](reference/mcp-servers/companies.md) | Companies server — valuation, forecasting, and portfolio analysis. |
| [`reference/mcp-servers/corpus.md`](reference/mcp-servers/corpus.md) | Corpus server — gather → process → output pipeline. |
| [`reference/mcp-servers/media.md`](reference/mcp-servers/media.md) | Media server — gallery, generation, transcription, jobs, and workflows. |
| [`reference/mcp-servers/portfolio.md`](reference/mcp-servers/portfolio.md) | Portfolio server — transaction-ledger portfolio store. |
| [`reference/mcp-servers/prediction-markets.md`](reference/mcp-servers/prediction-markets.md) | Prediction-markets server — Polymarket/Kalshi calibration and economic data. |
| [`reference/mcp-servers/research.md`](reference/mcp-servers/research.md) | Research server — web/RSS retrieval, evidence scoring, run ledger, and paper identity. |
| [`reference/mcp-servers/scenarios.md`](reference/mcp-servers/scenarios.md) | Scenarios server — Schwartz/Tetlock pipeline. |
| [`reference/mcp-servers/swarm.md`](reference/mcp-servers/swarm.md) | Swarm server — Agent Bestiary World and local substrate (87 tools). |
| [`reference/skills/README.md`](reference/skills/README.md) | Registry of 77 skills, 67 template namespaces, and 323 `.j2` resources. |
| [`reference/kask-settings.md`](reference/kask-settings.md) | Kask settings and environment reference. |
| [`reference/lisp-eval-dialect.md`](reference/lisp-eval-dialect.md) | Sandboxed `lisp_eval` dialect and `form`/`env` contract. |
| [`reference/ontology-bridge.md`](reference/ontology-bridge.md) | Published-vocabulary bridge, exact term resolver, and `onto_anchor` ladder. |
| [`reference/lora-training-catalog.md`](reference/lora-training-catalog.md) | LoRA training method, gate, harness, and nine-tool catalog. |
| [`reference/upstream-rebase-process.md`](reference/upstream-rebase-process.md) | Upstream rebase process and removal principles for D1–D66. |

## Diagrams

| Document | Description |
| --- | --- |
| [`DIAGRAMS_INDEX.md`](DIAGRAMS_INDEX.md) | Registry of 101 current `DIAGRAM_ALIGNMENT` records representing 101 unique IDs; 32 records are in the five consolidated files and 69 are inline. |
| [`diagrams/architecture.md`](diagrams/architecture.md) | Cross-cutting architecture diagrams. |
| [`diagrams/kanban.md`](diagrams/kanban.md) | Kata-kanban state machines. |
| [`diagrams/swarm.md`](diagrams/swarm.md) | Swarm architecture and control loops. |
| [`diagrams/ui-widgets.md`](diagrams/ui-widgets.md) | Native UI-widget structures. |
| [`diagrams/mcp-dispatch.md`](diagrams/mcp-dispatch.md) | MCP runtime and tool-dispatch flows. |

The operator-retained Proposed LogiSheets plan contains one explicitly conceptual future-state Mermaid block. Under [`DOCUMENTATION_STANDARDS.md`](architecture/DOCUMENTATION_STANDARDS.md) §3 and §4.2, it is not an implementation registry entry until implementation begins.

## Research

| Document | Description |
| --- | --- |
| [`research/chunking-for-rag-research.md`](research/chunking-for-rag-research.md) | Prior-art study of RAG text-chunking patterns and reference models, and the corpus pipeline's alignment and gaps against them. Recommendations only — not implemented. |
| [`research/kanban-board-reference-models.md`](research/kanban-board-reference-models.md) | Reference models for kanban board naming/navigation (Wekan, Planka, Kan, Kanboard), the kata-kanban's alignment and gaps against them, and the implemented shaping/test plan. |

## Document lifecycle ledger

Git history is the archive of record. Every removed document names its active successor here; the current tree contains 67 files.

### Deleted 2026-08-28 (condensation — no formal role / stale / duplicative)

| Artifact | Successor |
| --- | --- |
| `plans/` (10 files) | Point-in-time plans and implemented designs: swarm plans → `diataxis/swarm_system/`; fact-checking design → `grounding-verify`/`falsifiability`/`hypothesis-framer` skills; memory plans → `architecture/memory-system-specification.md`; other completed or abandoned work → git history. |
| `explanation/` (13 files) | Durable content folded into `architecture/skills-and-composition.md`, `architecture/memory-system-specification.md`, `reference/mcp-servers/README.md`, and surviving Diataxis/reference coverage. |
| `architecture/AGENT_SYSTEM_PROMPT.md` | `architecture/skills-and-composition.md` Part I. |
| `architecture/memory-system-and-therapy.md` | `architecture/memory-system-specification.md` §§9–11. |
| `architecture/salience-specification.md` | Implemented `word_frequencies` surface in `diataxis/hkask-condenser/reference.md`. |
| `architecture/hkask-types-core-domain-split.md` | No accepted successor architecture; `hkask-types` remains one crate, with current coverage in `diataxis/hkask-types/`. |
| `architecture/core/scenarios-companies-bridge.md` | `reference/mcp-servers/companies.md` scenarios/companies bridge coverage. |
| `diagrams/` (25 of 28 files) | Five consolidated files under `diagrams/`; three memory diagrams moved into `architecture/memory-system-specification.md`. |
| `REFRESH_TRIAGE.md` | This lifecycle ledger. |

### Deleted 2026-09-09 (doc-update realignment)

| Artifact | Successor |
| --- | --- |
| `architecture/research-server-capability-plan.md` | Implemented record in `reference/mcp-servers/research.md` under Capability adoption record. |
| `reference/README.md` | This portal's Reference table. |
| `reference/upstream-removal-principles.md` | `reference/upstream-rebase-process.md` §9. |
| `diataxis/hkask-bridge-ontology/how-to.md` | `reference/ontology-bridge.md` under How to use the bridge and term resolver. |

### Deleted 2026-09-16 (tutorial folds and implemented inference plan)

The eight tutorial removals landed in `5881f1f406fafe12f4fc65a549dbf1eb0a62ca84`; each successor below is the retained task-oriented document for that set. The implemented inference plan was removed in `5322725bdbaa067580943c2bf33634c3640d8341`.

| Deleted artifact | Named successor |
| --- | --- |
| `diataxis/hkask-condenser/tutorial.md` | `diataxis/hkask-condenser/how-to.md` |
| `diataxis/hkask-inference/tutorial.md` | `diataxis/hkask-inference/how-to.md` |
| `diataxis/hkask-regulation/tutorial.md` | `diataxis/hkask-regulation/how-to.md` |
| `diataxis/hkask-storage/tutorial.md` | `diataxis/hkask-storage/how-to.md` |
| `diataxis/hkask-tool-port/tutorial.md` | `diataxis/hkask-tool-port/reference.md` |
| `diataxis/hkask-types/tutorial.md` | `diataxis/hkask-types/how-to.md` |
| `diataxis/kask_bridge/tutorial.md` | `diataxis/kask_bridge/how-to.md` |
| `diataxis/swarm_system/tutorial.md` | `diataxis/swarm_system/how-to.md` |
| `plans/inference-regulation-loop-completion-plan.md` | Implemented inference-resilience behavior in `kask/crates/kask_bridge/src/inference_resilience.rs` and current operational explanation in `kask/docs/diataxis/hkask-regulation/explanation.md`. |

### Deleted 2026-09-17 (point-in-time prediction-market reports)

| Deleted artifact | Named successor |
| --- | --- |
| `reports/prediction-markets/` (5 files) | Current implementation and tool surface in `reference/mcp-servers/prediction-markets.md`; git history remains the research archive. |
| `reports/gemba-loop-specification.md` | Active procedures in `.agents/skills/gemba-walk/SKILL.md` and `.agents/skills/algedonic-review/SKILL.md`; git history remains the design archive. |

### Verification gate

- [x] Six-field metadata plus `mds_categories` is present on every active or operator-retained Proposed document.
- [x] Current-state Mermaid alignment and the Proposed conceptual exception satisfy `DOCUMENTATION_STANDARDS.md`.
- [x] Internal links in the previously blocked hkask-tool-port documents target retained reference/how-to documents.
- [x] Diagram metadata has unique-ID/location registry parity.
- [x] Edited citations use full repository-relative paths.
- [x] Document count is 67 and remains fewer than 70.

## See also

- [`DIVERGENCE.md`](../../DIVERGENCE.md) — authoritative D1–D66 divergence manifest.
- [`diataxis/INDEX.md`](diataxis/INDEX.md) — retained Diataxis sets.
- [`DIAGRAMS_INDEX.md`](DIAGRAMS_INDEX.md) — Mermaid verification registry.
