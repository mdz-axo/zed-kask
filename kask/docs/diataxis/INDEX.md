---
title: "zed-kask Diataxis Documentation Index"
audience: [developers, architects, agents, operators]
last_updated: 2026-09-16
version: "1.3.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, composition, trust, lifecycle, curation]
---

# zed-kask Diataxis Documentation Index

This index lists 28 retained artifacts across 10 cross-cutting documentation
sets. Eight tutorials were folded into their surviving how-to, reference, or
explanation documents; `hkask-mcp-server/tutorial.md` is the one retained
tutorial. The count is the current filesystem result under
`kask/docs/diataxis/*/*.md` on 2026-09-15, excluding this index.

Artifacts are expected to carry repo-relative implementation evidence. This
index does not claim that every citation in every retained artifact has been
re-verified in this pass; the files edited on 2026-09-15 carry their own current
verification dates.

## Diataxis quadrant map

| Quadrant | Purpose | MDS category | Typical diagram |
| --- | --- | --- | --- |
| Tutorial | Learn a concept | lifecycle | step flow |
| How-to | Accomplish a task | composition | procedure flow |
| Reference | Look up a fact | domain | class or data model |
| Explanation | Understand why | trust and curation | state or sequence |

## Retained sets — 28 artifacts

| Set | Tutorial | How-to | Reference | Explanation |
| --- | --- | --- | --- | --- |
| [swarm_system](./swarm_system/) | folded | [How-to](./swarm_system/how-to.md) | [Reference](./swarm_system/reference.md) | [Explanation](./swarm_system/explanation.md) |
| [hkask-types](./hkask-types/) | folded | [How-to](./hkask-types/how-to.md) | [Reference](./hkask-types/reference.md) | [Explanation](./hkask-types/explanation.md) |
| [hkask-tool-port](./hkask-tool-port/) | folded | — | [Reference](./hkask-tool-port/reference.md) | [Explanation](./hkask-tool-port/explanation.md) |
| [hkask-storage](./hkask-storage/) | folded | [How-to](./hkask-storage/how-to.md) | [Reference](./hkask-storage/reference.md) | [Explanation](./hkask-storage/explanation.md) |
| [hkask-regulation](./hkask-regulation/) | folded | [How-to](./hkask-regulation/how-to.md) | [Reference](./hkask-regulation/reference.md) | [Explanation](./hkask-regulation/explanation.md) |
| [hkask-inference](./hkask-inference/) | folded | [How-to](./hkask-inference/how-to.md) | [Reference](./hkask-inference/reference.md) | [Explanation](./hkask-inference/explanation.md) |
| [hkask-condenser](./hkask-condenser/) | folded | [How-to](./hkask-condenser/how-to.md) | [Reference](./hkask-condenser/reference.md) | [Explanation](./hkask-condenser/explanation.md) |
| [hkask-mcp-server](./hkask-mcp-server/) | [Tutorial](./hkask-mcp-server/tutorial.md) | [How-to](./hkask-mcp-server/how-to.md) | [Reference](./hkask-mcp-server/reference.md) | [Explanation](./hkask-mcp-server/explanation.md) |
| [kask_bridge](./kask_bridge/) | folded | [How-to](./kask_bridge/how-to.md) | [Reference](./kask_bridge/reference.md) | [Explanation](./kask_bridge/explanation.md) |
| [media_panel](./media_panel/) | — | — | [Reference](./media_panel/reference.md) | — |

## Out of scope for additional per-crate sets

### MCP server crates — complete 11-server inventory

The 11 managed server crates are documented cross-cuttingly under
[`kask/docs/reference/mcp-servers/`](../reference/mcp-servers/README.md) rather
than receiving another per-crate set:

`hkask-mcp-companies`, `hkask-mcp-corpus`, `hkask-mcp-curator`,
`hkask-mcp-kata-kanban`, `hkask-mcp-media`, `hkask-mcp-portfolio`,
`hkask-mcp-prediction-markets`, `hkask-mcp-research`, `hkask-mcp-scenarios`,
`hkask-mcp-swarm`, and `hkask-mcp-training`.

The authoritative managed registry contains the same 11 IDs at
`kask/crates/kask_bridge/src/mcp_servers.rs:52-506`.

### hKask library and composition crates — complete 18-crate inventory

The composition-root inventory under `kask/crates/` is:

`hkask-bridge-ontology`, `hkask-condenser`, `hkask-email`,
`hkask-event-store`, `hkask-forecast`, `hkask-inference`, `hkask-keystore`,
`hkask-lisp`, `hkask-mcp`, `hkask-mcp-server`, `hkask-memory`,
`hkask-regulation`, `hkask-services-core`, `hkask-steer-core`, `hkask-storage`,
`hkask-tool-port`, `hkask-types`, and `kask_bridge`.

Eight have retained crate-named sets above; `hkask-bridge-ontology` is covered
by [`ontology-bridge.md`](../reference/ontology-bridge.md). The remaining small
support crates are covered by cross-cutting architecture/reference documents
and crate-local implementation context rather than additional Diataxis sets.
The workspace membership evidence is `Cargo.toml:273-290`.

Zed-side crates such as `crates/agent`, `crates/agent_ui`, `crates/zed`, and
`crates/media_panel` are documented here only where a zed-kask capability or
D-seam requires it. The authoritative divergence range is D1–D56, with retired
numbers retained and never reused (`DIVERGENCE.md:125-186`).

## Governing specifications

- [Documentation Standards](../architecture/DOCUMENTATION_STANDARDS.md)
- [Minimal Domain Specification](../architecture/core/MDS.md)
- [Canonical host architecture](../architecture/zed-host-architecture-plan.md)
- [Divergence manifest](../../../DIVERGENCE.md)

## See also

- [Documentation portal](../README.md)
- [Diagram registry](../DIAGRAMS_INDEX.md)
