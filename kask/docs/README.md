---
title: "zed-kask Documentation"
audience: [developers, architects, agents, operators]
last_updated: 2026-11-18
version: "2.12.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle, curation]
---

# zed-kask Documentation

> **zed-kask** is a minimal-divergence fork of the [Zed editor](https://zed.dev) with the hKask agent platform compiled in-process. The agent runtime, skills, Regulation nervous system, and sovereign memory run inside the editor as native surfaces; 11 managed MCP servers are launched as child processes over stdio by the governed `McpRuntime` (`kask/crates/kask_bridge/src/mcp_servers/catalog.rs:26-424@BUILT_IN_MCP_SERVERS`).

**Canonical reference:** [`architecture/zed-host-architecture-plan.md`](architecture/zed-host-architecture-plan.md) describes the composition root and current crate inventory. The authoritative integration surface is [`DIVERGENCE.md`](../../DIVERGENCE.md), which lists current and retired numbered seams (retired numbers are never reused).

**Pre-collapse hashes:** commit hashes cited in this corpus predate the 2026-10-27 history collapse (see [`DIVERGENCE.md`](../../DIVERGENCE.md), "History collapse and layered reconstruction") and reference erased pre-collapse history; they do not resolve in this repository. The seam register carries the durable record.

**Per-crate docs:** [`diataxis/INDEX.md`](diataxis/INDEX.md) lists 16 retained artifacts across 8 cross-cutting sets. Eight tutorials were folded on 2026-09-16; on 2026-09-28 the eight per-crate how-to documents were folded into their set references (each reference gained a Procedures section) and the hkask-tool-port explanation was folded into its reference (that set was later deleted with its crate on 2026-11-07, commit `9b6b648a20`); `diataxis/hkask-mcp-server/tutorial.md` is the one retained tutorial.

**Corpus size (measured 2026-11-19):** 71 Markdown documents and one live YAML inventory, 72 files total under `find kask/docs -type f | wc -l` — the recorded count for the §3 condensation discipline, checked voluntarily with `kask/scripts/check-docs-count-gate.sh` (advisory agent tool; unwired from push/CI enforcement by operator mandate 2026-10-31 — nothing blocks a push or fails CI on this number) — under both the formal **fewer-than-85** cap and the **75-file working target** (cap and target history: operator rulings 2026-09-30 and 2026-10-07, [`DOCUMENTATION_STANDARDS.md`](architecture/DOCUMENTATION_STANDARDS.md) §3; the target is condensation direction, not a gate). `principle-constraints.yaml` is a live governance inventory consumed by `kask/scripts/check-principle-constraints.sh`, not a deletion candidate.

## Repair and improvement plans

| Document | Status and purpose |
| --- | --- |
| [`hKask core and MCP repair plan`](plans/hkask-core-mcp-repair-improvement-plan.md) | Active, operator-authorized repair plan; no backward-compatibility requirements. §9 separates verified gallery/packaging slices from unfinished authority, cancellation, containment, attribution and recovery work; P6 separates skill evaluation (algedonic review with operator and Curator) from skill execution; P8 records the experimentation program's execution records — the reference model is promoted to [`research/experimentation-reference-model.md`](research/experimentation-reference-model.md) (2026-10-05) with the locked operator decisions. |
| [`EUREKA-2: media and educt improvement-research cycle`](plans/eureka2-media-educt-cycle-plan-2026-10-28.md) | Proposed — decisions D1–D8 TPM-decided and presented for functional veto; execution tracked on the Transcript bench board. |
| [`Private-Inference MCP Server: functional requirements (D-2)`](plans/private-inference-mcp-requirements-2026-10-30.md) | Proposed — FR-3 (private-thread routing) and FR-4 (memory privacy) remain in force and are cited by the code while the kask-side serving stack (FR-1, FR-2, FR-5) was superseded 2026-11-11; every requirement is a falsifiable outcome claim tied to P1–P12 or V1–V8. |
| [`Private-inference M5 — the recall/export filters`](plans/private-inference-m5-recall-export-filters-plan-2026-11-05.md) | Proposed — designs the open recall/export filter legs against the post-D100/FR-4 tree (M5-A read-side provenance landed; remaining legs await separate engagement). |

## Architecture

| Document | Description |
| --- | --- |
| [`zed-host-architecture-plan.md`](architecture/zed-host-architecture-plan.md) | **Canonical architecture** — numbered D-seam integration authority, composition root, 21-crate `kask/crates/` inventory, 11 MCP servers, deletion history. |
| [`standardized-artifact-storage.md`](architecture/standardized-artifact-storage.md) | **D28** — canonical path layout for persistent Kask databases (`data_dir`) and visible artifacts (`artifacts_dir`). |
| [`memory-system-specification.md`](architecture/memory-system-specification.md) | **Memory system specification** — dual-lane `curator.db` + `curator-private.db`, INT8 `vec0` schema, ingestion, recall, consolidation, decay, hygiene, sovereignty, and embedded diagrams. |
| [`skills-and-composition.md`](architecture/skills-and-composition.md) | **Agent system** — five prompt surfaces (base + four overlays), skill body injection, composition principles, and testing. |
| [`functional-interaction-spec.md`](architecture/functional-interaction-spec.md) | **Functional interaction design record** — the Division of Responsibilities / four-moves agreement (prompt-side implementation retired 2026-10-31); §9's user–Curator dyad levels remain live. |
| [`compaction-pipeline-spec.md`](architecture/compaction-pipeline-spec.md) | **Compaction pipeline specification** — ratified 2026-09-29: the three entry points and the always-succeed, deterministic-stage-first pipeline contract. |
| [`core/PRINCIPLES.md`](architecture/core/PRINCIPLES.md) | Architecture principles P1–P12. |
| [`core/magna-carta.md`](architecture/core/magna-carta.md) | The Magna Carta — four sovereignty principles and their `principle-constraints.yaml` enforcement ledger. |
| [`core/magnifica-humanitas.md`](architecture/core/magnifica-humanitas.md) | Magnifica Humanitas — the values charter grounding P1–P12 in human dignity (source: Leo XIV, 2026). |
| [`core/co-research-environment.md`](architecture/core/co-research-environment.md) | The Co-Research Environment — the thesis charter: what zed-kask is (identity, anatomy, lineage, collaboration test); completes the charter triad with the Magna Carta (liberties) and Magnifica Humanitas (values). |
| [`core/MDS.md`](architecture/core/MDS.md) | Minimal Domain Specification — five-category taxonomy, 21 library/composition crates, and 11 MCP servers. |
| [`DOCUMENTATION_STANDARDS.md`](architecture/DOCUMENTATION_STANDARDS.md) | Metadata, lifecycle, Mermaid alignment, citation, and writing standards, including operator-retained Proposed plans. |

## Reference

| Document | Description |
| --- | --- |
| [`reference/testing-protocol.md`](reference/testing-protocol.md) | User expectation contracts, test-layer assignment, bounded proofs, and test-evidence validation. |
| [`reference/verification-ladder.md`](reference/verification-ladder.md) | The five verification rungs (presence, liveness, truth, grounding, binding), their zed-kask homes and clocks, the provenance lattice, and the open rung gaps. |
| [`reference/regulation-spans.md`](reference/regulation-spans.md) | Regulation tracing, persisted records, and actual MCP outcome paths. |
| [`reference/mcp-servers/README.md`](reference/mcp-servers/README.md) | MCP server registry — 11 built-in servers, 243 registered tools fleet-wide (reconciled 2026-11-18 against the `tool_surface_pin!` tests), and the fleet tool surface. |
| [`reference/mcp-servers/corpus.md`](reference/mcp-servers/corpus.md) | Corpus server — gather → process → output pipeline (24 tools). |
| [`reference/mcp-servers/curator.md`](reference/mcp-servers/curator.md) | Curator server — sovereign memory, regulation readback, and the algedonic log (16 tools). |
| [`reference/mcp-servers/experimentation.md`](reference/mcp-servers/experimentation.md) | Experimentation server — experiment registry for the sharded experimentation program (8 tools). |
| [`reference/mcp-servers/kata-kanban.md`](reference/mcp-servers/kata-kanban.md) | Kata-kanban server — Toyota Kata task boards and persistent-until-resolved functional goals (26 tools). |
| [`reference/mcp-servers/media.md`](reference/mcp-servers/media.md) | Media server — research-evidence ingestion, gallery, transcription, educt, media layers, voice, and reading-image fetch (91 tools). |
| [`reference/mcp-servers/nostr-blossom.md`](reference/mcp-servers/nostr-blossom.md) | Nostr Blossom server — local content-addressed Blossom media sidecar with reference-counted deletes (3 tools). |
| [`reference/mcp-servers/nostr-client.md`](reference/mcp-servers/nostr-client.md) | Nostr Client server — Nostr reads, editor-signed publish, and Blossom archive/mirror (10 tools). |
| [`reference/mcp-servers/nostr-relay.md`](reference/mcp-servers/nostr-relay.md) | Nostr Relay server — loopback LMDB event archive for allowlisted authors (4 tools). |
| [`reference/mcp-servers/prediction-markets.md`](reference/mcp-servers/prediction-markets.md) | Prediction-markets server — Polymarket/Kalshi calibration, economic data, EQM, and Decisions API MCDA (28 tools). |
| [`reference/mcp-servers/research.md`](reference/mcp-servers/research.md) | Research server — web retrieval, evidence scoring, run ledger, and paper identity (11 tools). |
| [`reference/mcp-servers/scenarios.md`](reference/mcp-servers/scenarios.md) | Scenarios server — Schwartz/Tetlock pipeline and equity forecast loop (22 tools). |
| [`reference/skills/README.md`](reference/skills/README.md) | Registry of 58 skills, 53 template namespaces, and 215 `.j2` resources. |
| [`loop-register.md`](loop-register.md) | Loop audit register — 25 system loops with audit verdicts, three-layer (A/B/C) classifications, INV1–INV6 alignment verdicts, and the change-log ledger. |
| [`reference/kask-settings.md`](reference/kask-settings.md) | Kask settings and environment reference. |
| [`reference/lisp-eval-dialect.md`](reference/lisp-eval-dialect.md) | Sandboxed `lisp_eval` dialect and `form`/`env` contract. |
| [`reference/ontology-bridge.md`](reference/ontology-bridge.md) | Published-vocabulary bridge, exact term resolver, and `onto_anchor` ladder. |
| [`reference/native-ontologies.md`](reference/native-ontologies.md) | Native-ontologies inventory — 89 pinned files in `sources/SOURCES.lock`, the 225-ruling derived registry, and the ladder tiers in functional terms; companion to `ontology-bridge.md`. |
| [`reference/upstream-rebase-process.md`](reference/upstream-rebase-process.md) | Upstream rebase process and removal principles for the numbered D-seams. |
| [`reference/agent-trap-history.md`](reference/agent-trap-history.md) | Archived `.rules` technical/protocol trap reference (consulted on demand, not context-injected). |

## Diagrams

| Document | Description |
| --- | --- |
| [`DIAGRAMS_INDEX.md`](DIAGRAMS_INDEX.md) | Registry of 74 current `DIAGRAM_ALIGNMENT` records representing 74 unique IDs; 17 records are in the four consolidated diagram files and 57 are inline. |
| [`diagrams/architecture.md`](diagrams/architecture.md) | Cross-cutting architecture diagrams. |
| [`diagrams/kanban.md`](diagrams/kanban.md) | Kata-kanban state machines: task status, move controller, and the goal lifecycle (create → progress → human closure → assessment → current recall). |
| [`diagrams/ui-widgets.md`](diagrams/ui-widgets.md) | Native UI-widget structures and prediction-markets pipeline. |
| [`diagrams/mcp-dispatch.md`](diagrams/mcp-dispatch.md) | MCP runtime and tool-dispatch flows. |

## Research

| Document | Description |
| --- | --- |
| [`research/d-seam-audit.md`](research/d-seam-audit.md) | D-seam audit ledger — resumable per-seam worklist over the live `DIVERGENCE.md` surface; identity + counts coverage checked by `kask/scripts/check-d-seam-audit-ledger.sh`. |
| [`research/experimentation-reference-model.md`](research/experimentation-reference-model.md) | The experimentation program's reference model: the vary-locally/select-centrally/retain-durably pattern, generalized experiment protocol, locked operator rulings (Q0–Q7), and `DIAG-ARCH-EUREKA-001`. |
| [`research/goal-reference-models.md`](research/goal-reference-models.md) | The goal system's canonical anchor-model record (six anchor models, schema-evolution contract, boundary pins). |
| [`research/kanban-board-reference-models.md`](research/kanban-board-reference-models.md) | Kanban board reference models (Wekan, Planka, Kan, Kanboard — R1–R8 invariants and gap table). |
| [`research/curator-nostr-client-design-2026-11-11.md`](research/curator-nostr-client-design-2026-11-11.md) | Curator Nostr client design authority and key-isolation threat model (D106). |
| [`research/curator-nostr-relay-design-2026-11-11.md`](research/curator-nostr-relay-design-2026-11-11.md) | Curator Nostr loopback relay design authority and admission perimeter (D105). |
| [`research/tree-sitter-adoption-proposal.md`](research/tree-sitter-adoption-proposal.md) | Tree-sitter code-graph capability research, ast-grep structural gates, and `@symbol` citation resolver. |

## Verification gate

- [x] Seven-field YAML frontmatter (`title`, `audience`, `last_updated`, `version`, `status`, `domain`, `mds_categories`) with normalized canonical `status` (`Active`, `Draft`, or `Proposed`) is present on every Markdown document (`check-docs-frontmatter.sh` green, 2026-11-18).
- [x] Current-state Mermaid alignment satisfies `DOCUMENTATION_STANDARDS.md` §4 (`74 fences = 74 records = 74 index rows`, zero `TOMBSTONED` table rows, zero inline HTML).
- [x] Internal links target retained documents — zero broken relative links across all 69 Markdown files.
- [x] Diagram metadata has unique-ID/location registry parity (74 records, tree-verified 2026-11-18).
- [x] Citations use full repository-relative paths with `@symbol` anchors where resolvable (`0 rotted`, `0 line-drift warnings`, `0 ambiguous-name warnings`, `0 @sym mismatches`, 2026-11-18).
- [x] Document count is 72 in the working tree (measured 2026-11-19: 71 Markdown plus the live `principle-constraints.yaml`), under both the fewer-than-85 cap and the 75-file working target.

## See also

- [`DIVERGENCE.md`](../../DIVERGENCE.md) — authoritative numbered divergence manifest.
- [`diataxis/INDEX.md`](diataxis/INDEX.md) — retained Diataxis sets.
- [`DIAGRAMS_INDEX.md`](DIAGRAMS_INDEX.md) — Mermaid verification registry.
