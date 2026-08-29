---
title: "zed-kask Documentation"
audience: [developers, architects, agents, operators]
last_updated: 2026-08-28
version: "2.0.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle, curation]
---

# zed-kask Documentation

> **zed-kask** is a minimal-divergence fork of the [Zed editor](https://zed.dev) with the hKask agent platform compiled in-process. The agent runtime, skills, Regulation nervous system, and sovereign memory run inside the editor as native surfaces; the 11 MCP servers are launched as child processes over stdio by zed's `context_server` host (`BUILT_IN_MCP_SERVERS`, `kask/crates/kask_bridge/src/mcp_servers.rs:55-431`).

**Canonical reference:** [`architecture/zed-host-architecture-plan.md`](architecture/zed-host-architecture-plan.md) — the D1–D38 integration plan, composition root, and current crate inventory. The authoritative divergence surface is [`DIVERGENCE.md`](../../DIVERGENCE.md) at the repo root.

**Per-crate docs:** [`diataxis/INDEX.md`](diataxis/INDEX.md) — Diataxis documentation set (tutorial, how-to, reference, explanation) for 10 cross-cutting crate sets (36 artifacts).

## Architecture

| Document | Description |
| --- | --- |
| [`zed-host-architecture-plan.md`](architecture/zed-host-architecture-plan.md) | **Canonical architecture** — D1–D33 integration seams, composition root, crate inventory, deletion history. |
| [`standardized-artifact-storage.md`](architecture/standardized-artifact-storage.md) | **D28** — the canonical path layout for all persistent kask artifacts (memory DBs, curator DBs, MCP server DBs, skills registry, archived threads). |
| [`memory-system-specification.md`](architecture/memory-system-specification.md) | **Memory system spec** — schema, ingestion, recall, consolidation, decay, hygiene tools, sovereignty, design rationale, embedded diagrams. |
| [`skills-and-composition.md`](architecture/skills-and-composition.md) | **Agent system** — the four prompt surfaces and their upstream divergences; skill anatomy, body-injection model, composition principles, testing. |
| [`core/PRINCIPLES.md`](architecture/core/PRINCIPLES.md) | Architecture principles P1–P12. |
| [`core/magna-carta.md`](architecture/core/magna-carta.md) | The Magna Carta — 4 sovereignty principles (P1–P4). |
| [`core/MDS.md`](architecture/core/MDS.md) | Minimal Domain Specification (5-category taxonomy, Composition Root: 18 surviving crates, 11 MCP servers). |
| [`DOCUMENTATION_STANDARDS.md`](architecture/DOCUMENTATION_STANDARDS.md) | Documentation standards (frontmatter, Mermaid-First, Sourced-Ideas, Writing Excellence). |

## Reference

| Document | Description |
| --- | --- |
| [`reference/regulation-spans.md`](reference/regulation-spans.md) | Regulation span catalog. |
| [`reference/mcp-servers/README.md`](reference/mcp-servers/README.md) | MCP server registry — 11 built-in servers, 362 `#[tool]` methods fleet-wide, forecasting-stack overview. |
| [`reference/mcp-servers/companies.md`](reference/mcp-servers/companies.md) | Companies server — valuation, forecasting, portfolio. |
| [`reference/mcp-servers/corpus.md`](reference/mcp-servers/corpus.md) | Corpus server — gather→process→output pipeline. |
| [`reference/mcp-servers/media.md`](reference/mcp-servers/media.md) | Media server — gallery, image/video/audio generation and processing, jobs, workflows (67 tools). |
| [`reference/mcp-servers/portfolio.md`](reference/mcp-servers/portfolio.md) | Portfolio server — transaction-ledger portfolio store. |
| [`reference/mcp-servers/prediction-markets.md`](reference/mcp-servers/prediction-markets.md) | Prediction-markets server — Polymarket/Kalshi calibration, economic data. |
| [`reference/mcp-servers/scenarios.md`](reference/mcp-servers/scenarios.md) | Scenarios server — Schwartz/Tetlock pipeline. |
| [`reference/mcp-servers/swarm.md`](reference/mcp-servers/swarm.md) | Swarm server — Agent Bestiary World agent swarms, Xaman Ek curator, local substrate (82 tools). |
| [`reference/skills/README.md`](reference/skills/README.md) | Skill, template, and bundle registry — 65 skills, body-injection model. |
| [`reference/kask-settings.md`](reference/kask-settings.md) | Kask settings reference. |
| [`reference/ontology-bridge.md`](reference/ontology-bridge.md) | Ontology bridge API reference. |
| [`reference/lora-training-catalog.md`](reference/lora-training-catalog.md) | LoRA training method/gate/harness catalog. |
| [`reference/upstream-rebase-process.md`](reference/upstream-rebase-process.md) | Upstream rebase management process. |
| [`reference/upstream-removal-principles.md`](reference/upstream-removal-principles.md) | Upstream-Zed removal principles for the seam. |

## Diagrams

| Document | Description |
| --- | --- |
| [`DIAGRAMS_INDEX.md`](DIAGRAMS_INDEX.md) | Mermaid diagram verification registry (31 diagram IDs across 5 consolidated files + inline diagrams). |
| [`diagrams/architecture.md`](diagrams/architecture.md) | Cross-cutting architecture: CMP research pipeline, constraint-forces skills, ontology bridge, skill/MCP/Lisp seam, credential resolution, tool port, event store, viz core. |
| [`diagrams/kanban.md`](diagrams/kanban.md) | Kata-kanban: task status and move-controller state machines. |
| [`diagrams/swarm.md`](diagrams/swarm.md) | Swarm: architecture, feedback loops, PDCA cascade, steering sequence, panel modes, server class. |
| [`diagrams/ui-widgets.md`](diagrams/ui-widgets.md) | UI widgets: graph, kanban, portfolio, prediction-markets, scenarios, swarm. |
| [`diagrams/mcp-dispatch.md`](diagrams/mcp-dispatch.md) | MCP tool dispatch: runtime invoke flow, tool-call sequence, CMP tool-call flow. |

## Document lifecycle ledger

Per `DOCUMENTATION_STANDARDS.md` §3 (Lifecycle). The 2026-08-28 condensation
reduced the tree from 120 to 69 documents (cap: <70). Every deletion is
recorded here with its successor; git history preserves full content.

### Deleted 2026-08-28 (condensation — no formal role / stale / duplicative)

| Artifact | Successor |
| --- | --- |
| `plans/` (10 files) | Point-in-time plans and implemented designs: swarm plans → `diataxis/swarm_system/`; fact-checking design → `grounding-verify`/`falsifiability`/`hypothesis-framer` skills; memory plans → `architecture/memory-system-specification.md`; thread-hooks refactor (IMPLEMENTED), audits, deferred Nebius draft → git history. |
| `explanation/` (13 files) | `skills-and-composition.md` → `architecture/`; `memory-system.md` → folded into `architecture/memory-system-specification.md`; `forecasting-and-scenarios.md` → folded into `reference/mcp-servers/README.md`; the rest duplicated diataxis/reference coverage or linked to non-existent diagrams → git history. |
| `architecture/AGENT_SYSTEM_PROMPT.md` | Folded into `architecture/skills-and-composition.md` (Part I). |
| `architecture/memory-system-and-therapy.md` | Folded into `architecture/memory-system-specification.md` (§9–§11). |
| `architecture/salience-specification.md` | Described an unimplemented salience model; the implemented surface (`word_frequencies`) is documented in `diataxis/hkask-condenser/reference.md`. |
| `architecture/hkask-types-core-domain-split.md` | Draft ADR, never accepted; `hkask-types` remains one crate. |
| `architecture/core/scenarios-companies-bridge.md` | Folded into `reference/mcp-servers/companies.md` (§ Scenarios ↔ Companies Bridge). |
| `diagrams/` (25 of 28 files) | Consolidated into 5 domain files (see Diagrams above); 3 memory diagrams folded into `architecture/memory-system-specification.md`. |
| `REFRESH_TRIAGE.md` | Superseded by this ledger. |

### Verification gate

- [x] Six-field metadata header present and correct on all active files
- [x] `mds_categories` field present with ≥1 category
- [x] Every Mermaid block has `DIAGRAM_ALIGNMENT` metadata
- [x] All internal links resolve
- [x] No aspirational content in `architecture/`
- [x] `last_updated` reflects final edit date (2026-08-28)
- [x] Document count < 70 (69)

## See also

- [`DIVERGENCE.md`](../../DIVERGENCE.md) — the fork's divergence manifest and upstream-sync runbook (repo root).
- [`diataxis/INDEX.md`](diataxis/INDEX.md) — per-crate Diataxis documentation set.
- [`DIAGRAMS_INDEX.md`](DIAGRAMS_INDEX.md) — cross-cutting Mermaid diagram registry.