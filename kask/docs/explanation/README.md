---
title: "Explanation — Architecture and Design Decisions"
audience: [architects, developers]
last_updated: 2026-07-20
version: "0.31.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, curation]
---

# Explanation — Architecture and Design Decisions

Background, context, and reasoning for hKask's design. "This design exists because…"


| Guide | Topics | Domain Tier |
|-------|-------|-------------|
| [Architecture Patterns](architecture-patterns.md) | Hexagonal ports and adapters (17 traits), loom-and-thread philosophy, Good Regulator theorem (Conant-Ashby), Viable System Model mapping (S1–S5), dual-axis ontology (PKO + DC+BIBO), API surface equivalence (P3). | Core |
| [Terminal UI Architecture](tui-architecture.md) | Ratatui workspace ownership, CLI→REPL→TUI bridge wiring, current implementation status, adversarial architecture findings, and componentized improvement order. | Domain supplement |
| [Regulation and Loops](regulation-and-loops.md) | Regulation homeostatic loop (sense→compare→compute→act→verify), skill PDCA model (FlowDef manifests, convergence contracts), Curator metacognition (CurationLoop + MetacognitionLoop), bug hunting methodology (Weinberg, Beizer, Hendrickson), QA system (YAML manifests, LLM classification, Regulation spans). | Core |
| [Sovereignty and OCAP](sovereignty-and-ocap.md) | Object Capability MCP dispatch (DelegationToken, GovernedTool 6-step membrane, fail-closed semantics), Diataxis quality review (diagram audit gates, OWASP anchoring). | Core |
| [Energy and Economy](energy-and-economy.md) | Gas system (GasBudget, Well, WalletManager, rJoule), double-entry ledger, database driver abstraction (SQLite/PostgreSQL, SQLCipher, column-level encryption), LoRA adapter store lifecycle (AdapterStore, AdapterRouter, EndpointLifecycle, provider selection). | Core |
| [Cognition and Replica](cognition-and-replica.md) | Fusion system design recommendations (multi-model deliberation), scenario forecasting (Schwartz + Tetlock + Chermack pipeline), ν-event semantics (ObservableSpan, RegulationRecord, CANONICAL_NAMESPACES, decay-weighted replay), Companies MCP server (41 tools, DCF valuation, forecast feedback, portfolio ledger). | Core |

## ADR Archive

Design decisions recorded as Architecture Decision Records:

- [ADR-031: Consolidation Authorization](../architecture/ADRs/ADR-031-consolidation-authorization.md)
- [ADR-035: UserPod Server Mode](../architecture/ADRs/ADR-035-userpod-server-mode.md)
- [ADR-043: Database Driver](../architecture/ADRs/ADR-043-database-driver.md)

> See `docs/architecture/ADRs/` for the full archive (active + retired ADRs).