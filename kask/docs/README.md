---
title: "zed-kask Documentation"
audience: [developers, architects, agents, operators]
last_updated: 2026-09-28
version: "2.6.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle, curation]
---

# zed-kask Documentation

> **zed-kask** is a minimal-divergence fork of the [Zed editor](https://zed.dev) with the hKask agent platform compiled in-process. The agent runtime, skills, Regulation nervous system, and sovereign memory run inside the editor as native surfaces; 12 managed MCP servers are launched as child processes over stdio by Zed's `context_server` host (`kask/crates/kask_bridge/src/mcp_servers.rs:55-547`).

**Canonical reference:** [`architecture/zed-host-architecture-plan.md`](architecture/zed-host-architecture-plan.md) describes the composition root and current crate inventory. The authoritative integration surface is [`DIVERGENCE.md`](../../DIVERGENCE.md), which lists current and retired numbered seams (retired numbers are never reused).

**Per-crate docs:** [`diataxis/INDEX.md`](diataxis/INDEX.md) lists 19 retained artifacts across 10 cross-cutting sets. Eight tutorials were folded on 2026-09-16; on 2026-09-28 the eight per-crate how-to documents were folded into their set references (each reference gained a Procedures section) and the hkask-tool-port explanation was folded into its reference; `diataxis/hkask-mcp-server/tutorial.md` is the one retained tutorial.

**Corpus size (measured 2026-09-28):** 64 Markdown documents, one live YAML inventory, and two plain-text evidence files, 67 files total (`find kask/docs -type f | wc -l`), under the formal **fewer-than-70** count gate in [`DOCUMENTATION_STANDARDS.md`](architecture/DOCUMENTATION_STANDARDS.md) §3. The 2026-09-28 condensation folded the eight per-crate how-to documents into their set references (Procedures sections), recorded in the lifecycle ledger below. [`principle-constraints.yaml`](architecture/principle-constraints.yaml) is a live governance inventory consumed by `kask/scripts/check-principle-constraints.sh`, not a deletion candidate.

## Repair and improvement plans

| Document | Status and purpose |
| --- | --- |
| [`hKask core and MCP repair plan`](plans/hkask-core-mcp-repair-improvement-plan.md) | Active, operator-authorized repair plan; no backward-compatibility requirements. §9 separates verified training/gallery/packaging slices from unfinished authority, cancellation, containment, attribution and recovery work; P6 separates skill evaluation (algedonic review with operator and Curator) from skill execution. |

## Architecture

| Document | Description |
| --- | --- |
| [`zed-host-architecture-plan.md`](architecture/zed-host-architecture-plan.md) | **Canonical architecture** — numbered D-seam integration authority, composition root, crate inventory, deletion history. |
| [`standardized-artifact-storage.md`](architecture/standardized-artifact-storage.md) | **D28** — canonical path layout for persistent Kask artifacts. |
| [`memory-system-specification.md`](architecture/memory-system-specification.md) | **Memory system specification** — schema, ingestion, recall, consolidation, decay, hygiene, sovereignty, and embedded diagrams. |
| [`skills-and-composition.md`](architecture/skills-and-composition.md) | **Agent system** — prompt surfaces, skill body injection, composition principles, and testing. |
| [`functional-interaction-spec.md`](architecture/functional-interaction-spec.md) | **Division of Responsibilities** — operator/product-manager and agent/program-manager working agreement. |
| [`core/PRINCIPLES.md`](architecture/core/PRINCIPLES.md) | Architecture principles P1–P12. |
| [`core/magna-carta.md`](architecture/core/magna-carta.md) | The Magna Carta — four sovereignty principles. |
| [`core/MDS.md`](architecture/core/MDS.md) | Minimal Domain Specification — five-category taxonomy, 19 library/composition crates, and 12 MCP servers. |
| [`DOCUMENTATION_STANDARDS.md`](architecture/DOCUMENTATION_STANDARDS.md) | Metadata, lifecycle, Mermaid alignment, citation, and writing standards, including operator-retained Proposed plans. |

## Reference

| Document | Description |
| --- | --- |
| [`reference/testing-protocol.md`](reference/testing-protocol.md) | User expectation contracts, test-layer assignment, bounded proofs, and test-evidence validation. |
| [`reference/regulation-spans.md`](reference/regulation-spans.md) | Regulation tracing, persisted records, and actual MCP outcome paths. |
| [`reference/mcp-servers/README.md`](reference/mcp-servers/README.md) | MCP server registry — 12 built-in servers and the fleet tool surface. |
| [`reference/mcp-servers/companies.md`](reference/mcp-servers/companies.md) | Companies server — valuation, forecasting, and portfolio analysis. |
| [`reference/mcp-servers/corpus.md`](reference/mcp-servers/corpus.md) | Corpus server — gather → process → output pipeline. |
| [`reference/mcp-servers/media.md`](reference/mcp-servers/media.md) | Media server — gallery, generation, transcription, jobs, and workflows. |
| [`reference/mcp-servers/portfolio.md`](reference/mcp-servers/portfolio.md) | Portfolio server — transaction-ledger portfolio store. |
| [`reference/mcp-servers/prediction-markets.md`](reference/mcp-servers/prediction-markets.md) | Prediction-markets server — Polymarket/Kalshi calibration and economic data. |
| [`reference/mcp-servers/research.md`](reference/mcp-servers/research.md) | Research server — web/RSS retrieval, evidence scoring, run ledger, and paper identity. |
| [`reference/mcp-servers/scenarios.md`](reference/mcp-servers/scenarios.md) | Scenarios server — Schwartz/Tetlock pipeline (19 tools). |
| [`reference/mcp-servers/spreadsheet.md`](reference/mcp-servers/spreadsheet.md) | Spreadsheet server — LogiSheets-backed workbook surface (2 tools). |
| [`reference/mcp-servers/swarm.md`](reference/mcp-servers/swarm.md) | Swarm server — Agent Bestiary World and local substrate (90 tools). |
| [`reference/skills/README.md`](reference/skills/README.md) | Registry of 60 skills, 55 template namespaces, and 272 `.j2` resources. |
| [`loop-register.md`](loop-register.md) | Loop audit register — 23 system loops with audit verdicts, deferred operator decisions, and the INVEST decomposition. |
| [`reference/kask-settings.md`](reference/kask-settings.md) | Kask settings and environment reference. |
| [`reference/lisp-eval-dialect.md`](reference/lisp-eval-dialect.md) | Sandboxed `lisp_eval` dialect and `form`/`env` contract. |
| [`reference/ontology-bridge.md`](reference/ontology-bridge.md) | Published-vocabulary bridge, exact term resolver, and `onto_anchor` ladder. |
| [`reference/lora-training-catalog.md`](reference/lora-training-catalog.md) | LoRA training method, gate, harness, and nine-tool catalog. |
| [`reference/upstream-rebase-process.md`](reference/upstream-rebase-process.md) | Upstream rebase process and removal principles for the numbered D-seams. |

## Diagrams

| Document | Description |
| --- | --- |
| [`DIAGRAMS_INDEX.md`](DIAGRAMS_INDEX.md) | Registry of 106 current `DIAGRAM_ALIGNMENT` records representing 106 unique IDs; 35 records are in the five consolidated files and 71 are inline. |
| [`diagrams/architecture.md`](diagrams/architecture.md) | Cross-cutting architecture diagrams. |
| [`diagrams/kanban.md`](diagrams/kanban.md) | Kata-kanban state machines: task status, move controller, and the goal lifecycle (create → judge → score → acknowledge outbox). |
| [`diagrams/swarm.md`](diagrams/swarm.md) | Swarm architecture and control loops. |
| [`diagrams/ui-widgets.md`](diagrams/ui-widgets.md) | Native UI-widget structures. |
| [`diagrams/mcp-dispatch.md`](diagrams/mcp-dispatch.md) | MCP runtime and tool-dispatch flows. |

The LogiSheets plan's future-state block converted to a registered implementation diagram (`DIAG-ARCH-SPREADSHEET-001`) when implementation began on 2026-09-18; `research/cmp-gap-methodology.md`'s Stage-7 routing block is registered (`DIAG-RES-CMP-001`) alongside the file's frontmatter (2026-09-19).

## Research

| Document | Description |
| --- | --- |
| [`research/artificial-curiosity-capability-space.md`](research/artificial-curiosity-capability-space.md) | Source-grounded curiosity mechanism study, separate seven-source follow-up, and bounded renderer-only capability-probe pilot; no tested curiosity selection capability. |
| [`research/chunking-for-rag-research.md`](research/chunking-for-rag-research.md) | Prior-art study of RAG text-chunking patterns and reference models, and the corpus pipeline's alignment and gaps against them. Recommendations only — not implemented. |
| [`research/kanban-board-reference-models.md`](research/kanban-board-reference-models.md) | Reference models for kanban board naming/navigation (Wekan, Planka, Kan, Kanboard), the kata-kanban's alignment and gaps against them, and the implemented shaping/test plan. |
| [`research/deterministic-vs-probabilistic-compute-routing.md`](research/deterministic-vs-probabilistic-compute-routing.md) | When deterministic vs probabilistic compute fits: routing claim with falsifiers, Shannon/Gibbs entropy analogy tested (Jaynes, Landauer bridges and breakdowns), and a problem matrix whose game axis splits into recurrence and world closure. Hypotheses only — no routing implemented. Added 2026-09-27. |
| [`research/d-seam-audit.md`](research/d-seam-audit.md) | D-seam audit ledger — resumable per-seam worklist over the live `DIVERGENCE.md` surface; identity coverage checked by `kask/scripts/check-d-seam-audit-ledger.sh`. |
| [`research/cmp-gap-methodology.md`](research/cmp-gap-methodology.md) | Process spec for detecting, measuring, and interpreting gaps between prediction-market and traditional-market expectations (CMP term structures vs rates/FX/equities analogs); worked run instances are recorded as companies-mcp reports. Added 2026-09-19 by the operator. |

## Document lifecycle ledger

Git history is the archive of record. Every removed document names its active successor here. The dated Corpus size measurement above, not this lifecycle ledger, applies the Markdown-document gate and identifies the separate live YAML inventory.

### Deleted 2026-09-28 (doc-update realignment — reference absorbs how-to)

| Artifact | Successor |
| --- | --- |
| `diataxis/swarm_system/how-to.md` | `diataxis/swarm_system/reference.md` (Procedures section). |
| `diataxis/hkask-types/how-to.md` | `diataxis/hkask-types/reference.md` (Procedures section). |
| `diataxis/hkask-storage/how-to.md` | `diataxis/hkask-storage/reference.md` (Procedures section). |
| `diataxis/hkask-regulation/how-to.md` | `diataxis/hkask-regulation/reference.md` (Procedures section). |
| `diataxis/hkask-inference/how-to.md` | `diataxis/hkask-inference/reference.md` (Procedures section). |
| `diataxis/hkask-condenser/how-to.md` | `diataxis/hkask-condenser/reference.md` (Procedures section). |
| `diataxis/kask_bridge/how-to.md` | `diataxis/kask_bridge/reference.md` (Procedures section). |
| `diataxis/hkask-mcp-server/how-to.md` | `diataxis/hkask-mcp-server/reference.md` (Procedures section). |
| `diataxis/hkask-tool-port/explanation.md` | `diataxis/hkask-tool-port/reference.md` (capability-separation rationale and the invoke-pipeline/boundary diagrams folded in). |
| `diataxis/hkask-bridge-ontology/` (empty directory) | Residue of the 2026-09-09 how-to fold; no files. |
| `reports/` (empty directory) | Residue of the 2026-09-17/19 report deletions; no files. |

### Folded 2026-09-23 (research and pilot evidence)

| Artifact | Successor |
| --- | --- |
| `research/artificial-curiosity-open-questions-followup.md` | `research/artificial-curiosity-capability-space.md` §§6, 8: distinct seven-source follow-up, limits and four-arm experiment design; full original remains in git at `aa7f6ce4eb`. |
| `research/artificial-curiosity-probe-pilot.md` | `research/artificial-curiosity-capability-space.md` §9: four positive/negative renderer controls, measured boundary and unresolved experiment; full original remains in git at `aa7f6ce4eb`. |

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
| `reports/gemba-loop-specification.md` | Active procedure in `.agents/skills/algedonic-review/SKILL.md` (its gemba walk phase; `gemba-walk` merged into it 2026-09-24); git history remains the design archive. |

### Deleted 2026-09-19 (doc-update realignment)

| Deleted artifact | Named successor |
| --- | --- |
| `reports/bug-hunt-report.json` | The bug-hunt skill's trace-filesystem expedition report (`.agents/skills/bug-hunt/SKILL.md:86` writes reports to the run trace dir); git history (added `ed9be534cc`, updated `8c92966ca6`, converged `be80e66916`) remains the durable archive of the 2026-09-18/19 expeditions. The docs-tree copy violated the `DOCUMENTATION_STANDARDS.md` §6.2 location policy (no `reports/` class) and pushed the tree to the 70-file cap. |
| `plans/goedel-gap-closure-plan.md` | Completed plan consolidated out: R1 (evaluation correctness), R2 (bounded proofs — predicate core verified, harness set removed `5b4799bcad`), R3 (Rust inventory checker), R4 (advisory goal-traceability), and the R5 approved acceptance cycle are delivered in code and tests; the acceptance protocol and Kani budget conventions live in [`reference/testing-protocol.md`](reference/testing-protocol.md); the remaining authority, activation, and recovery work is carried by [`plans/hkask-core-mcp-repair-improvement-plan.md`](plans/hkask-core-mcp-repair-improvement-plan.md) (P2/P3); the full plan and execution record remain in git history. |

### Verification gate

- [x] Six-field metadata plus `mds_categories` is present on every active or operator-retained Proposed document.
- [x] Current-state Mermaid alignment and the Proposed conceptual exception satisfy `DOCUMENTATION_STANDARDS.md`.
- [x] Internal links target retained documents; the eight folded how-to documents' links were repointed to their set references (2026-09-28).
- [x] Diagram metadata has unique-ID/location registry parity (106 records, tree-verified 2026-09-28).
- [x] Edited citations use full repository-relative paths; 525 sampled citations across the recomposed artifacts resolve (2026-09-28).
- [x] Document count is 67, under the fewer-than-70 cap (`find kask/docs -type f | wc -l`, measured 2026-09-28).

## See also

- [`DIVERGENCE.md`](../../DIVERGENCE.md) — authoritative numbered divergence manifest.
- [`diataxis/INDEX.md`](diataxis/INDEX.md) — retained Diataxis sets.
- [`DIAGRAMS_INDEX.md`](DIAGRAMS_INDEX.md) — Mermaid verification registry.
