---
title: "Explanation — Architecture and Design Decisions"
audience: [architects, developers]
last_updated: 2026-07-24
version: "0.31.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, curation]
---

# Explanation — Architecture and Design Decisions

Background, context, and reasoning for hKask's design as it runs in-process inside zed-kask. "This design exists because…"

hKask is compiled into zed-kask as a set of native crates and in-process MCP servers; the standalone `kask` CLI, HTTP API server, Matrix transport, and daemon process have been removed. The documents below describe the systems that survive that consolidation — Regulation, OCAP, skills, fusion, MCP servers — and how they plug into zed-kask's editor, agent panel, and inference path. For the canonical integration map, see [zed-kask Host Architecture Plan](../architecture/zed-host-architecture-plan.md) (seams D1–D10).

| Guide | Topics | Domain Tier |
|-------|-------|-------------|
| [Architecture Patterns](architecture-patterns.md) | Hexagonal ports and adapters (17 traits), loom-and-thread philosophy, Good Regulator theorem (Conant-Ashby), Viable System Model mapping (S1–S5), dual-axis ontology (PKO + DC+BIBO) | Core |
| [Regulation and Loops](regulation-and-loops.md) | Regulation homeostatic loop (sense→compare→compute→act→verify), skill PDCA model (FlowDef manifests, convergence contracts), Curator metacognition (CurationLoop + MetacognitionLoop) evaluating in-process agent events, bug hunting methodology (Weinberg, Beizer, Hendrickson), QA system (YAML manifests, LLM classification, Regulation spans). | Core |
| [Sovereignty and OCAP](sovereignty-and-ocap.md) | Object Capability MCP dispatch (DelegationToken, GovernedTool 6-step membrane, fail-closed semantics), Diataxis quality review (diagram audit gates, OWASP anchoring). | Core |
| [Sovereignty and Observability](sovereignty-and-observability.md) | Capability tokens, Regulation alerts, and the observability surfaces exposed in-process to zed-kask's agent panel. | Core |
| [Energy and Economy](energy-and-economy.md) | Gas system (GasBudget, Well, WalletManager, rJoule), double-entry ledger, database driver abstraction (SQLite/PostgreSQL, SQLCipher, column-level encryption), LoRA adapter store lifecycle (AdapterStore, AdapterRouter, EndpointLifecycle, provider selection). | Core |
| [Cognition and Replica](cognition-and-replica.md) | Fusion system design recommendations (multi-model deliberation), scenario forecasting (Schwartz + Tetlock + Chermack pipeline), ν-event semantics (ObservableSpan, RegulationRecord, CANONICAL_NAMESPACES, decay-weighted replay), Companies MCP server (41 tools, DCF valuation, forecast feedback, portfolio ledger). | Core |
| [Fusion Mode](fusion-mode.md) | The 5 LLM deliberation modes (synthesis, best-of-n, critique, deliberation, pi), the algo / no-judge path, per-skill manifest overrides, and how fusion is operated from the zed-kask agent panel. | Core |
| [Skills and Composition](skills-and-composition.md) | Skill anatomy (two-zone model), manifest authoring, the ManifestExecutor (D1) execution path, skill bundles, and how MCP servers register as in-process builtins inside the editor. | Core |
| [Companies MCP Server](companies-mcp.md) | How-to procedures for company valuation, forecasting, and portfolio analysis against the in-process companies MCP server. | Domain supplement |
| [Forecasting and Scenarios](forecasting-and-scenarios.md) | Three-layer forecasting architecture and the scenario planning pipeline. | Domain supplement |
| [Ontology-Anchored Embedding](ontology-anchored-embedding.md) | Embedding model selection, ontological anchoring, and the QA pipeline. | Domain supplement |
| [Training and Adapters](training-and-adapters.md) | RunPod/Unsloth LoRA training path for Qwen3.6-27B and adapter lifecycle via the in-process training MCP server. | Domain supplement |
| [RunPod LoRA Training Guide](runpod-lora-training-guide.md) | Step-by-step RunPod pod launch and Unsloth training execution. | Domain supplement |
| [Security Skills Smoke Test](security-skills-smoke-test.md) | Manual smoke-test procedures for the security skills (supply-chain-sentinel, kali-audit, runtime-posture-monitor, attack-taxonomy-mapper) invoked in-process from the agent panel. | Domain supplement |
