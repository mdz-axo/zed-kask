---
title: "zed-kask Documentation"
audience: [developers, architects, agents, operators]
last_updated: 2026-08-15
version: "0.35.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle, curation]
---

# zed-kask Documentation

> **zed-kask** is a minimal-divergence fork of the [Zed editor](https://zed.dev) with the hKask agent platform compiled in-process. The agent runtime, skills, Regulation nervous system, and sovereign memory run inside the editor as native surfaces; the 13 MCP servers are launched as child processes over stdio by zed's `context_server` host.

**Canonical reference:** [`architecture/zed-host-architecture-plan.md`](architecture/zed-host-architecture-plan.md) — the D1–D28 integration plan, composition root, and current crate inventory. The authoritative divergence surface is [`DIVERGENCE.md`](../../DIVERGENCE.md) at the repo root.

**Per-crate docs:** [`diataxis/INDEX.md`](diataxis/INDEX.md) — Diataxis documentation set (tutorial, how-to, reference, explanation) for 11 cross-cutting crate sets (40 artifacts).

## Architecture

| Document                                                                                | Description                                                                                                                                               |
| --------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [`zed-host-architecture-plan.md`](architecture/zed-host-architecture-plan.md)           | **Canonical architecture** — D1–D28 integration seams, composition root, crate inventory, deletion history.                                               |
| [`standardized-artifact-storage.md`](architecture/standardized-artifact-storage.md)     | **D28** — the canonical path layout for all persistent kask artifacts (memory DBs, curator DBs, MCP server DBs, skills registry, archived threads).       |
| [`memory-system-specification.md`](architecture/memory-system-specification.md)         | **Memory system spec** — vector + relational lookup, ingestion, recall, consolidation, decay, configuration.                                              |
| [`salience-specification.md`](architecture/salience-specification.md)                   | Passage salience algorithm for `hkask-memory` (`compute_salience_batch`).                                                                                 |
| [`AGENT_SYSTEM_PROMPT.md`](architecture/AGENT_SYSTEM_PROMPT.md)                         | The agent system prompt — structure and zed-kask divergence from upstream Zed.                                                                          |
| [`test-harness-trace-schema.md`](architecture/test-harness-trace-schema.md)             | Trace filesystem schema for the proptest/bug-hunt/harness-optimize skill pipeline.                                                                        |
| [`core/PRINCIPLES.md`](architecture/core/PRINCIPLES.md)                                 | Architecture principles P1–P12.                                                                                                                           |
| [`core/magna-carta.md`](architecture/core/magna-carta.md)                               | The Magna Carta — 4 sovereignty principles (P1–P4).                                                                                                       |
| [`core/MDS.md`](architecture/core/MDS.md)                                               | Minimal Domain Specification (5-category taxonomy).                                                                                                       |
| [`core/scenarios-companies-bridge.md`](architecture/core/scenarios-companies-bridge.md) | Bridge tool between scenarios and companies MCP servers.                                                                                                  |
| [`hkask-types-core-domain-split.md`](architecture/hkask-types-core-domain-split.md)     | **ADR (Draft)** — split `hkask-types` into core primitives vs domain types; options, trade-offs, audit gate.                                              |
| [`adr-embedded-yaml-registry.md`](architecture/adr-embedded-yaml-registry.md)           | **ADR (Active)** — build-time `include_str!` embedding of all YAML/Jinja2 artifacts; dev-scoped evolution vs user-scoped freeze; trust model interaction. |
| [`DOCUMENTATION_STANDARDS.md`](architecture/DOCUMENTATION_STANDARDS.md)                 | Documentation standards (frontmatter, Mermaid-First, Sourced-Ideas, Writing Excellence).                                                                   |

## Reference

| Document                                                                   | Description                                                             |
| -------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| [`reference/regulation-spans.md`](reference/regulation-spans.md)           | Regulation span catalog.                                                |
| [`reference/mcp-servers/README.md`](reference/mcp-servers/README.md)       | MCP server registry — 13 built-in servers, 308 tools fleet-wide.        |
| [`reference/mcp-servers/companies.md`](reference/mcp-servers/companies.md) | Companies server — valuation, forecasting, portfolio (44 tools).        |
| [`reference/mcp-servers/condenser.md`](reference/mcp-servers/condenser.md) | Condenser server — compression algorithms.                              |
| [`reference/mcp-servers/corpus.md`](reference/mcp-servers/corpus.md)       | Corpus server — gather→process→output pipeline.                         |
| [`reference/mcp-servers/portfolio.md`](reference/mcp-servers/portfolio.md) | Portfolio server — transaction-ledger portfolio store.                 |
| [`reference/mcp-servers/prediction-markets.md`](reference/mcp-servers/prediction-markets.md) | Prediction-markets server — Polymarket/Kalshi calibration.        |
| [`reference/mcp-servers/scenarios.md`](reference/mcp-servers/scenarios.md) | Scenarios server — Schwartz/Tetlock pipeline.                           |
| [`reference/mcp-servers/swarm.md`](reference/mcp-servers/swarm.md)         | Swarm server — Agent Bestiary World agent swarms, Xaman Ek curator, local substrate (52 tools). |
| [`reference/skills/README.md`](reference/skills/README.md)                 | Skill, template, and bundle registry — 64 skills, 1:1 with SKILL.md dirs. |
| [`reference/kask-settings.md`](reference/kask-settings.md)                 | Kask settings reference.                                                 |
| [`reference/ontology-bridge.md`](reference/ontology-bridge.md)             | Ontology bridge API reference.                                           |
| [`reference/lora-training-catalog.md`](reference/lora-training-catalog.md) | LoRA training method/gate/harness catalog.                              |

## Explanation

| Document                                                                                   | Description                                                                   |
| ------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------- |
| [`explanation/skills-and-composition.md`](explanation/skills-and-composition.md)           | Skill anatomy, invocation, composition, and the five composition principles (determinism frontier, persistence-grounded learning, failure surfacing, lisp scaffold, co-evolution loop).                                       |
| [`explanation/skill-mcp-integration.md`](explanation/skill-mcp-integration.md)           | How flowdef steps invoke MCP tools natively via `action: execute` — the two invocation patterns, the action dispatch table, and the three co-evolution feedback loops. |
| [`explanation/memory-system.md`](explanation/memory-system.md)                             | **Memory system** — why vector + relational, the entity_ref bug, decay model. |
| [`explanation/abw-swarm-orchestration.md`](explanation/abw-swarm-orchestration.md)         | Agent Bestiary World swarm orchestration design.                              |
| [`explanation/cognition-and-replica.md`](explanation/cognition-and-replica.md)             | Scenario forecasting, ν-event semantics, Companies server.                    |
| [`explanation/companies-mcp.md`](explanation/companies-mcp.md)                             | Companies MCP server how-to.                                                  |
| [`explanation/company-corpus-design.md`](explanation/company-corpus-design.md)             | Company corpus design (discovery → ontology-anchored KG → RAG).               |
| [`explanation/earnings-transcript-analysis-design.md`](explanation/earnings-transcript-analysis-design.md) | Earnings-call transcript analysis design.                       |
| [`explanation/forecasting-and-scenarios.md`](explanation/forecasting-and-scenarios.md)     | Forecasting across skill, library, and scenarios layers.                      |
| [`explanation/ontology-anchored-embedding.md`](explanation/ontology-anchored-embedding.md) | Tag→embed corpus pipeline.                                                    |
| [`explanation/training-and-adapters.md`](explanation/training-and-adapters.md)             | RunPod/Unsloth LoRA training path.                                            |
| [`explanation/runpod-lora-training-guide.md`](explanation/runpod-lora-training-guide.md)   | RunPod LoRA training lessons.                                                 |
| [`explanation/security-skills-smoke-test.md`](explanation/security-skills-smoke-test.md)   | Manual smoke-test procedure.                                                  |

## Plans

Build plans for major features. All plans in the active tree have `status: Active` or `status: Draft`; `Deprecated`/`Superseded` plans are removed per `DOCUMENTATION_STANDARDS.md` §3.

| Document                                                             | Description                                                                                                                                                                             |
| -------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [`plans/abw-swarm-intelligence.md`](plans/abw-swarm-intelligence.md) | Agent Bestiary World (ABW) swarm intelligence integration — `hkask-mcp-swarm` MCP server (52 tools: 27 ABW + 25 local) + `swarm_panel`. v1 feature-complete; v2 local mode implemented. |
| [`plans/cybernetic-swarm-plan.md`](plans/cybernetic-swarm-plan.md)   | Cybernetic Swarm Plan — the `swarm-intelligence` skill design + implementation record. 10-step PDCA cascade, C0–C8 cybernetic components, steering modes, `delegate_results` contract.  |
| [`plans/evolving-test-harness.md`](plans/evolving-test-harness.md)   | Evolving test harness plan — trace filesystem, oracle taxonomy, property-based testing infrastructure.                                                                                  |

## Other

| Document                                 | Description                            |
| ---------------------------------------- | -------------------------------------- |
| [`DIAGRAMS_INDEX.md`](DIAGRAMS_INDEX.md) | Mermaid diagram verification registry. |
| [`upstream-rebase-process.md`](upstream-rebase-process.md) | Upstream rebase management process. |
| [`upstream-removal-principles.md`](upstream-removal-principles.md) | Upstream-Zed removal principles for the seam. |

## See also

- [`DIVERGENCE.md`](../../DIVERGENCE.md) — the fork's divergence manifest and upstream-sync runbook (repo root).
- [`diataxis/INDEX.md`](diataxis/INDEX.md) — per-crate Diataxis documentation set.
- [`DIAGRAMS_INDEX.md`](DIAGRAMS_INDEX.md) — cross-cutting Mermaid diagram registry.
