---
title: "Reference Documentation — Index"
audience: [developers, operators, agents]
last_updated: 2026-08-04
version: "0.32.3"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain]
---

# Reference Documentation

Neutral, complete, descriptive-only documentation of the hKask system as it is hosted inside
zed-kask. No procedures, no opinions, no explanations of why — only what.

hKask runs in-process inside zed-kask: 19 kask crates (18 `hkask-*` + `kask_bridge`) compiled
into the editor and 13 MCP servers hosted on disk via zed's `context_server` infrastructure.
The standalone `kask` CLI, HTTP API server, Matrix transport, daemon process, and REPL surfaces
have been **deleted** and are not referenced here as current. See
[`docs/architecture/zed-host-architecture-plan.md`](../architecture/zed-host-architecture-plan.md)
for the D1–D28 integration seams and the essentialist split.

## Skill & Template Registry

- [Skill Registry Index](skills/README.md) — All skills + templates + bundles with FlowDef parameters

## Regulation Span Registry

- [Regulation Span Registry](regulation-spans.md) — Domain-specific `ObservableSpan` enums, emission points, algedonic thresholds

## MCP Servers

- [MCP Server Registry](mcp-servers/README.md) — All 13 on-disk MCP servers with tool tables and capability tiers
- [Companies MCP Server](mcp-servers/companies.md) — 44 tools, dual-provider routing, forecast store, portfolio ledger
- [Condenser MCP Server](mcp-servers/condenser.md) — 4 tools, 3 compression algorithms, 2-phase condensation
- [Corpus / DocProc MCP Server](mcp-servers/corpus.md) — Corpus gathering, document processing, QA generation, style replicas
- [Scenarios MCP Server](mcp-servers/scenarios.md) — Event-tree forecasting pipeline
- [Swarm MCP Server](mcp-servers/swarm.md) — Agent Bestiary World agent swarms, Xaman Ek curator, consent-gated spend

## LoRA Training

- [LoRA Training Catalog](lora-training-catalog.md) — Adapter catalog, training harness matrix, dataset inventory
