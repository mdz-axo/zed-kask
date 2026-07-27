---
title: "zed-kask Diataxis Documentation Index"
audience: [developers, architects, agents, operators]
last_updated: 2026-07-27
version: "0.1.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle, curation]
---

# zed-kask Diataxis Documentation Index

This index lists the per-crate Diataxis documentation set for zed-kask.
Each major crate has four artifacts: Tutorial (learning path), How-to
(procedural), Reference (informational), and Explanation (understanding).
Every artifact cites concrete file:line references in the current tree.

## Diataxis quadrant map

| Quadrant | Purpose | MDS Category | Diagram Type |
|----------|---------|--------------|--------------|
| Tutorial | Learn a concept | Lifecycle | Step-by-step flowchart |
| How-to | Accomplish a task | Composition | Procedural flowchart |
| Reference | Look up a fact | Domain | ERD or class diagram |
| Explanation | Understand why | Trust + Curation | State or sequence diagram |

## Major crates (10 crates, 40 artifacts)

| Crate | Tutorial | How-to | Reference | Explanation |
|-------|----------|--------|-----------|-------------|
| [hkask-types](./hkask-types/) | [Tutorial](./hkask-types/tutorial.md) | [How-to](./hkask-types/how-to.md) | [Reference](./hkask-types/reference.md) | [Explanation](./hkask-types/explanation.md) |
| [hkask-capability](./hkask-capability/) | [Tutorial](./hkask-capability/tutorial.md) | [How-to](./hkask-capability/how-to.md) | [Reference](./hkask-capability/reference.md) | [Explanation](./hkask-capability/explanation.md) |
| [hkask-storage](./hkask-storage/) | [Tutorial](./hkask-storage/tutorial.md) | [How-to](./hkask-storage/how-to.md) | [Reference](./hkask-storage/reference.md) | [Explanation](./hkask-storage/explanation.md) |
| [hkask-regulation](./hkask-regulation/) | [Tutorial](./hkask-regulation/tutorial.md) | [How-to](./hkask-regulation/how-to.md) | [Reference](./hkask-regulation/reference.md) | [Explanation](./hkask-regulation/explanation.md) |
| [hkask-inference](./hkask-inference/) | [Tutorial](./hkask-inference/tutorial.md) | [How-to](./hkask-inference/how-to.md) | [Reference](./hkask-inference/reference.md) | [Explanation](./hkask-inference/explanation.md) |
| [hkask-templates](./hkask-templates/) | [Tutorial](./hkask-templates/tutorial.md) | [How-to](./hkask-templates/how-to.md) | [Reference](./hkask-templates/reference.md) | [Explanation](./hkask-templates/explanation.md) |
| [hkask-condenser](./hkask-condenser/) | [Tutorial](./hkask-condenser/tutorial.md) | [How-to](./hkask-condenser/how-to.md) | [Reference](./hkask-condenser/reference.md) | [Explanation](./hkask-condenser/explanation.md) |
| [hkask-mcp-server](./hkask-mcp-server/) | [Tutorial](./hkask-mcp-server/tutorial.md) | [How-to](./hkask-mcp-server/how-to.md) | [Reference](./hkask-mcp-server/reference.md) | [Explanation](./hkask-mcp-server/explanation.md) |
| [kask_bridge](./kask_bridge/) | [Tutorial](./kask_bridge/tutorial.md) | [How-to](./kask_bridge/how-to.md) | [Reference](./kask_bridge/reference.md) | [Explanation](./kask_bridge/explanation.md) |
| [kask_panel](./kask_panel/) | [Tutorial](./kask_panel/tutorial.md) | [How-to](./kask_panel/how-to.md) | [Reference](./kask_panel/reference.md) | [Explanation](./kask_panel/explanation.md) |

## Out-of-scope crates (N/A)

| Crate(s) | Reason | Cross-cutting reference |
|----------|--------|------------------------|
| `hkask-mcp-companies`, `hkask-mcp-corpus`, `hkask-mcp-scenarios`, `hkask-mcp-condenser`, `hkask-mcp-curator`, `hkask-mcp-kata-kanban`, `hkask-mcp-media`, `hkask-mcp-research`, `hkask-mcp-codegraph`, `hkask-mcp-training` | Already documented cross-cuttingly in `kask/docs/reference/mcp-servers/` | [`reference/mcp-servers/README.md`](../reference/mcp-servers/README.md) |
| `hkask-goal`, `hkask-forecast`, `hkask-email`, `hkask-ledger`, `hkask-guard`, `hkask-keystore`, `hkask-memory`, `hkask-bridge-dublincore`, `hkask-services-*`, `hkask-mcp` | Small support crates (<3000 LOC); documented via crate READMEs and cross-cutting docs | [`architecture/zed-host-architecture-plan.md`](../architecture/zed-host-architecture-plan.md) |
| `crates/agent`, `crates/agent_ui`, `crates/zed`, etc. | Upstream zed crates, not zed-kask code; only `// zed-kask:` deviations documented under `kask_bridge` and `kask_panel` | [`architecture/zed-host-architecture-plan.md`](../architecture/zed-host-architecture-plan.md) |

## Governing specifications

- [`architecture/DOCUMENTATION_STANDARDS.md`](../architecture/DOCUMENTATION_STANDARDS.md): documentation standards (frontmatter, Mermaid-First, Sourced-Ideas, Writing Excellence).
- [`architecture/core/MDS.md`](../architecture/core/MDS.md): Minimal Domain Specification (5-category taxonomy).
- [`../../docs/.conventions/brand-voice/`](../../../docs/.conventions/brand-voice/): brand voice rubric and taboo phrases.

## See also

- [`kask/docs/README.md`](../README.md): the kask docs portal.
- [`kask/docs/architecture/zed-host-architecture-plan.md`](../architecture/zed-host-architecture-plan.md): D1–D10 integration seams.
- [`kask/docs/DIAGRAMS_INDEX.md`](../DIAGRAMS_INDEX.md): cross-cutting diagram registry.
