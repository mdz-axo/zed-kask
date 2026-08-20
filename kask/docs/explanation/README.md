---
title: "Explanation — Architecture and Design Decisions"
audience: [architects, developers]
last_updated: 2026-08-20
version: "0.37.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, curation]
---

# Explanation — Architecture and Design Decisions

Background, context, and reasoning for hKask's design as it runs in-process inside zed-kask. "This design exists because…"

hKask is compiled into zed-kask as a set of native crates, plus 10 MCP servers launched as child processes over stdio; the standalone `kask` CLI, HTTP API server, Matrix transport, and daemon process have been removed. The documents below describe the systems that survive that consolidation — Regulation, tool dispatch, skills, MCP servers — and how they plug into zed-kask's editor, agent panel, and inference path. For the canonical integration map, see [zed-kask Host Architecture Plan](../architecture/zed-host-architecture-plan.md) (seams D1–D28).

| Guide | Topics | Domain Tier |
|-------|-------|-------------|
| [Tool dispatch](../diataxis/hkask-tool-port/explanation.md) | The `ToolPort` dispatch seam: `McpRuntime::invoke` meters and dispatches but does **not** authorize; the runaway-loop call breaker; the three allowlist boundaries where tool authority is enforced; and defense Layer 5 (information flow control) absent by decision (RR-0053). | Core |
| [Cognition and Replica](cognition-and-replica.md) | Scenario forecasting (Schwartz + Tetlock + Chermack pipeline), ν-event semantics (ObservableSpan, RegulationRecord, CANONICAL_NAMESPACES, decay-weighted replay), Companies MCP server (44 tools, DCF valuation, forecast feedback, portfolio ledger). | Core |
| [Skills and Composition](skills-and-composition.md) | Skill anatomy (two-zone model), the upstream-Zed body-injection execution path (`SkillTool::run` → `render_skill_envelope`, D1), skill bundles, and how MCP servers register as builtins inside the editor (child processes over stdio). | Core |
| [Companies MCP Server](companies-mcp.md) | How-to procedures for company valuation, forecasting, and portfolio analysis against the companies MCP server (child process over stdio). | Domain supplement |
| [Forecasting and Scenarios](forecasting-and-scenarios.md) | Three-layer forecasting architecture and the scenario planning pipeline. | Domain supplement |
| [Ontology-Anchored Embedding](ontology-anchored-embedding.md) | Embedding model selection, ontological anchoring, and the QA pipeline. | Domain supplement |
| [Training and Adapters](training-and-adapters.md) | RunPod/Unsloth LoRA training path for Qwen3.6-27B and adapter lifecycle via the training MCP server (child process over stdio). | Domain supplement |
| [RunPod LoRA Training Guide](runpod-lora-training-guide.md) | Step-by-step RunPod pod launch and Unsloth training execution. | Domain supplement |
| [Security Skills Smoke Test](security-skills-smoke-test.md) | Manual smoke-test procedures for the security skills (supply-chain-sentinel, kali-audit, runtime-posture-monitor) invoked in-process from the agent panel. | Domain supplement |
| [ABW Swarm Orchestration](abw-swarm-orchestration.md) | Agent Bestiary World integration — the four-mode Agent Swarm panel, agent authoring, team composition via Xaman Ek, the consent-gated spend model, the algedonic wallet channel, and the `swarm-intelligence` composition PDCA. | Domain supplement |
| [Earnings Transcript Analysis Design](earnings-transcript-analysis-design.md) | Design exploration for earnings-call transcript analysis: FMP-sourced transcripts, the listening template, and the MAIA seam model. Now the `earnings` mode of the generalized `company_transcript` tool. | Domain supplement |
| [Company Corpus Design](company-corpus-design.md) | Company corpus discovery + approved-source tier manifest (MAIA self-description doctrine); generalized `company_transcript` (earnings/corpus/combined); ontology-anchored KG + centroids + RAG. | Domain supplement |
