---
title: "Explanation — Architecture and Design Decisions"
audience: [architects, developers]
last_updated: 2026-08-05
version: "0.34.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, curation]
---

# Explanation — Architecture and Design Decisions

Background, context, and reasoning for hKask's design as it runs in-process inside zed-kask. "This design exists because…"

hKask is compiled into zed-kask as a set of native crates and in-process MCP servers; the standalone `kask` CLI, HTTP API server, Matrix transport, and daemon process have been removed. The documents below describe the systems that survive that consolidation — Regulation, capability-match gate, skills, MCP servers — and how they plug into zed-kask's editor, agent panel, and inference path. For the canonical integration map, see [zed-kask Host Architecture Plan](../architecture/zed-host-architecture-plan.md) (seams D1–D20).

| Guide | Topics | Domain Tier |
|-------|-------|-------------|
| [Capability tokens and dispatch](../diataxis/hkask-capability/explanation.md) | Capability-token MCP dispatch (DelegationToken, `McpRuntime::invoke` / `ToolGovernance` membrane, capability-match gate + gas budgets; token signatures are NOT verified — in-process calls only). | Core |
| [Cognition and Replica](cognition-and-replica.md) | Scenario forecasting (Schwartz + Tetlock + Chermack pipeline), ν-event semantics (ObservableSpan, RegulationRecord, CANONICAL_NAMESPACES, decay-weighted replay), Companies MCP server (42 tools, DCF valuation, forecast feedback, portfolio ledger). | Core |
| [Skills and Composition](skills-and-composition.md) | Skill anatomy (two-zone model), manifest authoring, the `BridgeManifestExecutor` (D1) execution path, skill bundles, and how MCP servers register as in-process builtins inside the editor. | Core |
| [Companies MCP Server](companies-mcp.md) | How-to procedures for company valuation, forecasting, and portfolio analysis against the in-process companies MCP server. | Domain supplement |
| [Forecasting and Scenarios](forecasting-and-scenarios.md) | Three-layer forecasting architecture and the scenario planning pipeline. | Domain supplement |
| [Ontology-Anchored Embedding](ontology-anchored-embedding.md) | Embedding model selection, ontological anchoring, and the QA pipeline. | Domain supplement |
| [Training and Adapters](training-and-adapters.md) | RunPod/Unsloth LoRA training path for Qwen3.6-27B and adapter lifecycle via the in-process training MCP server. | Domain supplement |
| [RunPod LoRA Training Guide](runpod-lora-training-guide.md) | Step-by-step RunPod pod launch and Unsloth training execution. | Domain supplement |
| [Security Skills Smoke Test](security-skills-smoke-test.md) | Manual smoke-test procedures for the security skills (supply-chain-sentinel, kali-audit, runtime-posture-monitor) invoked in-process from the agent panel. | Domain supplement |
| [ABW Swarm Orchestration](abw-swarm-orchestration.md) | Agent Bestiary World integration — the four-mode Agent Swarm panel, agent authoring, team composition via Xaman Ek, the consent-gated spend model, the algedonic wallet channel, and the `swarm-intelligence` composition PDCA. | Domain supplement |
| [Earnings Transcript Analysis Design](earnings-transcript-analysis-design.md) | Design exploration for earnings-call transcript analysis: FMP-sourced transcripts, the listening template, and the MAIA seam model. | Domain supplement |
