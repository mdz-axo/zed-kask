---
title: "zed-kask Documentation"
audience: [developers, architects, agents, operators]
last_updated: 2026-08-04
version: "0.3.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle, curation]
---

# zed-kask Documentation

> **zed-kask** is a minimal-divergence fork of the [Zed editor](https://zed.dev) with the hKask agent platform compiled in-process. The agent runtime, MCP servers, skills, Regulation nervous system, and sovereign memory run inside the editor as native surfaces.

**Canonical reference:** [`architecture/zed-host-architecture-plan.md`](architecture/zed-host-architecture-plan.md) — the D1–D20 integration plan, composition root, and current crate inventory.

**Per-crate docs:** [`diataxis/INDEX.md`](diataxis/INDEX.md) — Diataxis documentation set (tutorial, how-to, reference, explanation) for 10 crate sets (39 artifacts).

## Architecture

| Document | Description |
|----------|-------------|
| [`zed-host-architecture-plan.md`](architecture/zed-host-architecture-plan.md) | **Canonical architecture** — D1–D20 integration seams, composition root, crate inventory, deletion history. |
| [`salience-specification.md`](architecture/salience-specification.md) | Passage salience algorithm for `hkask-memory` (`compute_salience_batch`). |
| [`core/PRINCIPLES.md`](architecture/core/PRINCIPLES.md) | Architecture principles P1–P12. |
| [`core/magna-carta.md`](architecture/core/magna-carta.md) | The Magna Carta — 4 sovereignty principles (P1–P4). |
| [`core/scenarios-companies-bridge.md`](architecture/core/scenarios-companies-bridge.md) | Bridge tool between scenarios and companies MCP servers. |
| [`hkask-types-core-domain-split.md`](architecture/hkask-types-core-domain-split.md) | **ADR (Draft)** — split `hkask-types` into core primitives vs domain types; options, trade-offs, audit gate. |
| [`adr-embedded-yaml-registry.md`](architecture/adr-embedded-yaml-registry.md) | **ADR (Active)** — build-time `include_str!` embedding of all YAML/Jinja2 artifacts; dev-scoped evolution vs user-scoped freeze; trust model interaction. |

## Reference

| Document | Description |
|----------|-------------|
| [`reference/regulation-spans.md`](reference/regulation-spans.md) | Regulation span catalog. |
| [`reference/mcp-servers/README.md`](reference/mcp-servers/README.md) | MCP server registry — 11 built-in servers (child processes over stdio). |
| [`reference/mcp-servers/companies.md`](reference/mcp-servers/companies.md) | Companies server — valuation, forecasting, portfolio. |
| [`reference/mcp-servers/condenser.md`](reference/mcp-servers/condenser.md) | Condenser server — compression algorithms. |
| [`reference/mcp-servers/corpus.md`](reference/mcp-servers/corpus.md) | Corpus server — gather→process→output pipeline. |
| [`reference/mcp-servers/scenarios.md`](reference/mcp-servers/scenarios.md) | Scenarios server — Schwartz/Tetlock pipeline. |
| [`reference/mcp-servers/swarm.md`](reference/mcp-servers/swarm.md) | Swarm server — Agent Bestiary World agent swarms, Xaman Ek curator. |
| [`reference/skills/README.md`](reference/skills/README.md) | Skill registry. |
| [`reference/lora-training-catalog.md`](reference/lora-training-catalog.md) | LoRA training method/gate/harness catalog. |

## Explanation

| Document | Description |
|----------|-------------|
| [`explanation/skills-and-composition.md`](explanation/skills-and-composition.md) | Skill anatomy, invocation, composition. |
| [`explanation/cognition-and-replica.md`](explanation/cognition-and-replica.md) | Scenario forecasting, ν-event semantics, Companies server. |
| [`explanation/companies-mcp.md`](explanation/companies-mcp.md) | Companies MCP server design. |
| [`explanation/forecasting-and-scenarios.md`](explanation/forecasting-and-scenarios.md) | Forecasting across skill, library, and scenarios layers. |
| [`explanation/ontology-anchored-embedding.md`](explanation/ontology-anchored-embedding.md) | Tag→embed corpus pipeline. |
| [`explanation/training-and-adapters.md`](explanation/training-and-adapters.md) | RunPod/Unsloth LoRA training path. |
| [`explanation/runpod-lora-training-guide.md`](explanation/runpod-lora-training-guide.md) | RunPod LoRA training lessons. |
| [`explanation/security-skills-smoke-test.md`](explanation/security-skills-smoke-test.md) | Manual smoke-test procedure. |
| [`explanation/abw-swarm-orchestration.md`](explanation/abw-swarm-orchestration.md) | Agent Bestiary World swarm orchestration design. |

## Research

Historical research reports. Archived research has been removed from the active tree per `DOCUMENTATION_STANDARDS.md` §3 (lifecycle: Deprecated → Removed → git history is the archive of record). Recoverable via `git log --diff-filter=D -- kask/docs/research/archive/`.

| Document | Description |
|----------|-------------|
| [`research/media-research/media-landscape.md`](research/media-research/media-landscape.md) | Media tools → models → provider endpoints. |
| [`research/media-research/design-schema.md`](research/media-research/design-schema.md) | Media MCP server gallery schema. |

## Other

| Document | Description |
|----------|-------------|
| [`DIAGRAMS_INDEX.md`](DIAGRAMS_INDEX.md) | Mermaid diagram verification registry. |

## Plans

Build plans for major features. All plans in the active tree have `status: Active` or `status: Draft`; `Deprecated`/`Superseded` plans are removed per `DOCUMENTATION_STANDARDS.md` §3.

| Document | Description |
|----------|-------------|
| [`plans/kask-skill-signing-and-trust.md`](plans/kask-skill-signing-and-trust.md) | Skill marketplace signing & trust model — Ed25519-signed manifests with `expires_at` set at signing, server verification (upload 400 / poll skip), 120-day catalog expiry + sweep, client install verification. All 5 phases complete. |
| [`plans/abw-swarm-intelligence.md`](plans/abw-swarm-intelligence.md) | Agent Bestiary World (ABW) swarm intelligence integration — `hkask-mcp-swarm` MCP server (50 tools: 27 ABW + 23 local) + `swarm_panel`. v1 feature-complete; v2 local mode implemented. |
| [`plans/cybernetic-swarm-plan.md`](plans/cybernetic-swarm-plan.md) | Cybernetic Swarm Plan — the `swarm-intelligence` skill design + implementation record. 10-step PDCA cascade, C0–C8 cybernetic components, steering modes, `delegate_results` contract. |
| [`plans/media-system-refactor.md`](plans/media-system-refactor.md) | Media system review, architecture refactor, and multi-provider expansion plan. Forward-looking; not yet executed. |
| [`plans/prediction-plan.md`](plans/prediction-plan.md) | Prediction markets MCP server plan — proposed `hkask-mcp-prediction-markets` crate. Forward-looking. |
| [`plans/semantic-memory-wiki.md`](plans/semantic-memory-wiki.md) | Semantic memory wiki — proposed `hkask-mcp-wiki` MCP server. Draft; no code exists yet. |
| [`plans/evolving-test-harness.md`](plans/evolving-test-harness.md) | Evolving test harness plan — trace filesystem, oracle taxonomy, property-based testing infrastructure. |
| [`plans/test-harness-skill-evolution.md`](plans/test-harness-skill-evolution.md) | Test harness skill evolution — proptest/bug-hunt/tdd skill integration plan. |

## QA

Quality assurance strategy and artifacts for the hKask MCP server fleet.

| Document | Description |
|----------|-------------|
| [`qa/mcp-server-qa-strategy.md`](qa/mcp-server-qa-strategy.md) | Per-tool QA routine for all tools across 11 MCP servers. |
| [`qa/per-tool-contracts.md`](qa/per-tool-contracts.md) | Per-tool 7-category contract tables (input struct, output shape, LLM I/O boundary). |
| [`qa/coverage-matrix.md`](qa/coverage-matrix.md) | Coverage matrix generated by `scripts/qa-mcp-servers.sh`. |
| [`qa/pre-release-final-summary-pass3.md`](qa/pre-release-final-summary-pass3.md) | Final pre-release QA summary — 5 open deferred/user-decision items (D2, D3, D6, D7, D8). |
| [`qa/skill-bundle.yaml`](qa/skill-bundle.yaml) | Skill-bundle manifest for the QA pass (consumed by `skill-bundler`). |

## Audits

| Document | Description |
|----------|-------------|
| [`audits/swarm-cybernetics-semantics-audit.md`](audits/swarm-cybernetics-semantics-audit.md) | Swarm system pragmatic-semantics + pragmatic-cybernetics audit — 4 unresolved structural findings. |

## Status

Structural inventories and status snapshots.

| Document | Description |
|----------|-------------|
| [`status/public-seam-inventory.json`](status/public-seam-inventory.json) | Structural inventory of `kask/crates/` lib roots (20 crates). API-surface enumeration is a follow-up. |
