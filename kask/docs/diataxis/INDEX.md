---
title: "zed-kask Diataxis Documentation Index"
audience: [developers, architects, agents, operators]
last_updated: 2026-08-28
version: "1.2.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle, curation]
---

# zed-kask Diataxis Documentation Index

This index lists the per-crate Diataxis documentation set for zed-kask.
Each major crate has up to four artifacts: Tutorial (learning path), How-to
(procedural), Reference (informational), and Explanation (understanding).
Every artifact cites concrete file:line references in the current tree.

## Diataxis quadrant map

| Quadrant    | Purpose           | MDS Category     | Diagram Type              |
| ----------- | ----------------- | ---------------- | ------------------------- |
| Tutorial    | Learn a concept   | Lifecycle        | Step-by-step flowchart    |
| How-to      | Accomplish a task | Composition      | Procedural flowchart      |
| Reference   | Look up a fact    | Domain           | ERD or class diagram      |
| Explanation | Understand why    | Trust + Curation | State or sequence diagram |

## Major crates (10 cross-cutting sets, 36 artifacts)

| Crate                                             | Tutorial                                   | How-to                                      | Reference                                    | Explanation                                      |
| ------------------------------------------------- | ------------------------------------------ | ------------------------------------------- | -------------------------------------------- | ------------------------------------------------ |
| [swarm_system](./swarm_system/)                   | [Tutorial](./swarm_system/tutorial.md)     | [How-to](./swarm_system/how-to.md)          | [Reference](./swarm_system/reference.md)     | [Explanation](./swarm_system/explanation.md)     |
| [hkask-types](./hkask-types/)                     | [Tutorial](./hkask-types/tutorial.md)      | [How-to](./hkask-types/how-to.md)           | [Reference](./hkask-types/reference.md)      | [Explanation](./hkask-types/explanation.md)      |
| [hkask-tool-port](./hkask-tool-port/)           | [Tutorial](./hkask-tool-port/tutorial.md) | —                                           | [Reference](./hkask-tool-port/reference.md) | [Explanation](./hkask-tool-port/explanation.md) |
| [hkask-storage](./hkask-storage/)                 | [Tutorial](./hkask-storage/tutorial.md)    | [How-to](./hkask-storage/how-to.md)         | [Reference](./hkask-storage/reference.md)    | [Explanation](./hkask-storage/explanation.md)    |
| [hkask-regulation](./hkask-regulation/)           | [Tutorial](./hkask-regulation/tutorial.md) | [How-to](./hkask-regulation/how-to.md)      | [Reference](./hkask-regulation/reference.md) | [Explanation](./hkask-regulation/explanation.md) |
| [hkask-inference](./hkask-inference/)             | [Tutorial](./hkask-inference/tutorial.md)  | [How-to](./hkask-inference/how-to.md)       | [Reference](./hkask-inference/reference.md)  | [Explanation](./hkask-inference/explanation.md)  |

| [hkask-condenser](./hkask-condenser/)             | [Tutorial](./hkask-condenser/tutorial.md)  | [How-to](./hkask-condenser/how-to.md)       | [Reference](./hkask-condenser/reference.md)  | [Explanation](./hkask-condenser/explanation.md)  |
| [hkask-mcp-server](./hkask-mcp-server/)           | [Tutorial](./hkask-mcp-server/tutorial.md) | [How-to](./hkask-mcp-server/how-to.md)      | [Reference](./hkask-mcp-server/reference.md) | [Explanation](./hkask-mcp-server/explanation.md) |
| [kask_bridge](./kask_bridge/)                     | [Tutorial](./kask_bridge/tutorial.md)      | [How-to](./kask_bridge/how-to.md)           | [Reference](./kask_bridge/reference.md)      | [Explanation](./kask_bridge/explanation.md)      |
| [hkask-bridge-ontology](./hkask-bridge-ontology/) | —                                          | [How-to](./hkask-bridge-ontology/how-to.md) | [Reference](../reference/ontology-bridge.md) | —                                                |

## Out-of-scope crates (N/A)

| Crate(s)                                                                                                                                                                                                                                       | Reason                                                                                                                                                                                                                                                                                                                                              | Cross-cutting reference                                                                       |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `hkask-mcp-companies`, `hkask-mcp-corpus`, `hkask-mcp-scenarios`, `hkask-mcp-curator`, `hkask-mcp-kata-kanban`, `hkask-mcp-research`, `hkask-mcp-swarm`, `hkask-mcp-training` | Already documented cross-cuttingly in `kask/docs/reference/mcp-servers/`. | [`reference/mcp-servers/README.md`](../reference/mcp-servers/README.md)                       |
| `hkask-forecast`, `hkask-email`, `hkask-ledger`, `hkask-keystore`, `hkask-memory`, `hkask-services-core`, `hkask-mcp`, `hkask-lisp`                                                                                                     | Small support crates (<3000 LOC); documented via cross-cutting docs (and crate READMEs where present — `hkask-email` has none). Note: the other `hkask-services-*` crates were folded into their MCP server consumers (F6 refactor). | [`architecture/zed-host-architecture-plan.md`](../architecture/zed-host-architecture-plan.md) |

| `crates/agent`, `crates/agent_ui`, `crates/zed`, etc.                                                                                                                                                                                          | Upstream zed crates, not zed-kask code; only `// zed-kask:` deviations documented under `kask_bridge` and `swarm_panel`                                                                                                                                                                                                                             | [`architecture/zed-host-architecture-plan.md`](../architecture/zed-host-architecture-plan.md) |

## Governing specifications

- [`architecture/DOCUMENTATION_STANDARDS.md`](../architecture/DOCUMENTATION_STANDARDS.md): documentation standards (frontmatter, Mermaid-First, Sourced-Ideas, Writing Excellence).
- [`architecture/core/MDS.md`](../architecture/core/MDS.md): Minimal Domain Specification (5-category taxonomy).
- [`docs/.conventions/brand-writer/`](../../../docs/.conventions/brand-writer/): brand voice rubric and taboo phrases.

## See also

- [`kask/docs/README.md`](../README.md): the kask docs portal.
- [`kask/docs/architecture/zed-host-architecture-plan.md`](../architecture/zed-host-architecture-plan.md): D1–D32 integration seams (D21 = conversation-injector, D22 = block-reachability pins, D23 = worktree spawn wiring; D24–D32 added 2026-08-05 through 2026-08-20 — see `DIVERGENCE.md` for the authoritative seam list).
- [`DIVERGENCE.md`](../../../DIVERGENCE.md): the authoritative divergence surface (D1–D32).
- [`kask/docs/DIAGRAMS_INDEX.md`](../DIAGRAMS_INDEX.md): cross-cutting diagram registry.
