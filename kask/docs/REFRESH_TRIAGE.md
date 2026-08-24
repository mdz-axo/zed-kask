---
title: "Docs Refresh Triage Table — 2026-08-24"
audience: [documentation steward, architects]
last_updated: 2026-08-24
version: "0.1.0"
status: "Active"
domain: "documentation"
mds_categories: [curation, lifecycle]
---

# Docs Refresh Triage Table

Per `DOCUMENTATION_STANDARDS.md` §3 (Lifecycle) and §10 (Verification checklist).
Every pre-existing file under `kask/docs/` is accounted for. No orphans, no
silent deletions.

## Summary counts

- Total files inventoried: 113
- Active (keep-refresh): 99
- Draft (keep-refresh): 1
- Superseded (delete per §3): 1
- Missing metadata header (refresh to add): 10
- Invalid status value (refresh to normalize): 3
- Broken README links (fix): 2
- New diagrams required (Composition Root gaps): 2

## Triage table

| Path | Current status | Recommended action | §3 / §10 reason |
|------|---------------|--------------------|----|
| `README.md` | Active | keep-refresh | Fix broken links to `upstream-rebase-process.md` and `upstream-removal-principles.md` (now in `reference/`). §8 cross-reference rule. |
| `DIAGRAMS_INDEX.md` | Active | keep-refresh | Add `hkask-swarm-widget` and `hkask-event-store` diagram rows. §4.2 registry completeness. |
| `architecture/AGENT_SYSTEM_PROMPT.md` | Active | keep-refresh | Verify `file:line` refs still resolve. §10. |
| `architecture/DOCUMENTATION_STANDARDS.md` | Active | keep-refresh | `last_updated` 2026-08-01 stale vs 2026-08-24 refresh. §2. |
| `architecture/adr-embedded-yaml-registry.md` | Superseded | **delete** | §3: "Superseded → `git rm`; successor carries the content forward." Successor is body-injection model documented in `AGENT_SYSTEM_PROMPT.md` and `explanation/skills-and-composition.md`. |
| `architecture/core/MDS.md` | Active | keep-refresh | Add `hkask-event-store` to Composition Root (crate exists, wired via `kask_bridge/src/rollout_event_bridge.rs`, missing from table). §11.3. |
| `architecture/core/PRINCIPLES.md` | Active | keep-refresh | Verify P-numbers and refs. §10. |
| `architecture/core/magna-carta.md` | Active | keep-refresh | Verify refs. §10. |
| `architecture/core/scenarios-companies-bridge.md` | Active | keep-refresh | Verify `file:line` refs. §10. |
| `architecture/hkask-types-core-domain-split.md` | Draft | keep-refresh | ADR Draft — verify options still valid. §3 Draft state. |
| `architecture/memory-system-specification.md` | Active | keep-refresh | Verify schema refs. §10. |
| `architecture/salience-specification.md` | Active | keep-refresh | Verify `compute_salience_batch` ref. §10. |
| `architecture/standardized-artifact-storage.md` | **MISSING** | keep-refresh | Add §2 metadata header. Verify `agent_paths.rs` refs. §2 mandatory. |
| `architecture/thinking-cleanup-plan.md` | **MISSING** | keep-refresh | Add §2 metadata header. Verify execution-plan status (Phase 1 marked DONE). §2 mandatory. |
| `architecture/zed-host-architecture-plan.md` | Active | keep-refresh | Add `hkask-event-store` + `hkask-swarm-widget` to crate inventory. §10. |
| `diagrams/architecture-cmp-research-pipeline.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/architecture-constraint-forces-skills.md` | **MISSING** | keep-refresh | Add §2 metadata header + §4.2 DIAGRAM_ALIGNMENT. Diagram references undefined `GRAPH` node (fix). §2 + §4.2. |
| `diagrams/architecture-ontology-bridge.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/architecture-skill-mcp-lisp-seam.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/class-hkask-graph-widget.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/class-hkask-kanban-widget.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/class-hkask-portfolio-widget.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/class-hkask-prediction-markets.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/class-hkask-scenarios-widget.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/class-hkask-tool-port.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/class-hkask-viz-core.md` | Active | keep-refresh | Add `hkask-swarm-widget` to viz-core registry diagram (6th widget now wired). §4.2. |
| `diagrams/class-swarm-server.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/erd-credential-resolution.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/erd-memory-store.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/flowchart-cmp-tool-call-flow.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/flowchart-mcp-runtime-invoke.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/flowchart-memory-recall.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/flowchart-swarm-architecture.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/flowchart-swarm-feedback-loops.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/flowchart-swarm-pdca-cascade.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/sequence-mcp-tool-call.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/sequence-memory-ingest.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/sequence-swarm-steering-loop.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/state-kanban-move-controller.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/state-swarm-panel-modes.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diagrams/state-task-status.md` | Active | keep-refresh | Verify refs. §4.2. |
| `diataxis/INDEX.md` | Active | keep-refresh | Verify 9-crate set still accurate. §10. |
| `diataxis/hkask-bridge-ontology/*` (1 file) | Active | keep-refresh | Verify refs. §10. |
| `diataxis/hkask-condenser/*` (4 files) | Active | keep-refresh | Verify refs. §10. |
| `diataxis/hkask-inference/*` (4 files) | Active | keep-refresh | Verify refs. §10. |
| `diataxis/hkask-mcp-server/*` (4 files) | Active | keep-refresh | Verify refs. §10. |
| `diataxis/hkask-regulation/*` (4 files) | Active | keep-refresh | Verify refs. §10. |
| `diataxis/hkask-storage/*` (4 files) | Active | keep-refresh | Verify refs. §10. |
| `diataxis/hkask-tool-port/*` (3 files) | Active | keep-refresh | Verify refs. §10. |
| `diataxis/hkask-types/*` (4 files) | Active | keep-refresh | Verify refs. §10. |
| `diataxis/kask_bridge/*` (4 files) | Active | keep-refresh | Verify refs. §10. |
| `diataxis/swarm_system/*` (4 files) | Active | keep-refresh | Verify refs. §10. |
| `explanation/README.md` | Active | keep-refresh | Verify index. §10. |
| `explanation/abw-swarm-orchestration.md` | Active | keep-refresh | Verify refs. §10. |
| `explanation/cognition-and-replica.md` | Active | keep-refresh | Verify refs. §10. |
| `explanation/companies-mcp.md` | Active | keep-refresh | Verify refs. §10. |
| `explanation/company-corpus-design.md` | **invalid status** (`implemented (slices...)`) | keep-refresh | Normalize `status` to `Active` (per §2: exactly one of four values). §2. |
| `explanation/earnings-transcript-analysis-design.md` | **MISSING** | keep-refresh | Add §2 metadata header. Design doc — `status: Active`. §2 mandatory. |
| `explanation/forecasting-and-scenarios.md` | Active | keep-refresh | Verify refs. §10. |
| `explanation/memory-system.md` | Active | keep-refresh | Verify refs. §10. |
| `explanation/ontology-anchored-embedding.md` | Active | keep-refresh | Verify refs. §10. |
| `explanation/runpod-lora-training-guide.md` | Active | keep-refresh | Verify refs. §10. |
| `explanation/security-skills-smoke-test.md` | Active | keep-refresh | Verify refs. §10. |
| `explanation/skill-mcp-integration.md` | Active | keep-refresh | Verify refs. §10. |
| `explanation/skills-and-composition.md` | Active | keep-refresh | Verify refs. §10. |
| `explanation/training-and-adapters.md` | Active | keep-refresh | Verify refs. §10. |
| `plans/abw-swarm-intelligence.md` | **invalid status** (`Partially Deprecated`) | keep-refresh | Normalize `status` to `Active` or `Superseded` (per §2: exactly one of four values). Plan is feature-complete per content. §2. |
| `plans/cybernetic-swarm-plan.md` | **invalid status** (`Partially Deprecated`) | keep-refresh | Normalize `status` to `Active` or `Superseded`. §2. |
| `plans/event-substrate-proposal.md` | **MISSING** | keep-refresh | Add §2 metadata header. `status: Draft` (proposal, no code). §2 mandatory. |
| `plans/fact-checking-design.md` | **MISSING** (has prose `Status:` block) | keep-refresh | Convert prose `Status:` block to §2 YAML header. `status: Active` (implemented). §2 mandatory. |
| `plans/nebius-serverless-inference.md` | **MISSING** (has prose `## Status`) | keep-refresh | Convert prose `## Status` to §2 YAML header. `status: Draft` (deferred). §2 mandatory. |
| `plans/rules-proposal-inference-providers-default-divergence.md` | **MISSING** (has prose `**Status:**`) | keep-refresh | Convert prose `**Status:**` to §2 YAML header. `status: Draft` (proposal). §2 mandatory. |
| `reference/README.md` | Active | keep-refresh | Verify index. §10. |
| `reference/kask-settings.md` | Active | keep-refresh | Verify refs. §10. |
| `reference/lora-training-catalog.md` | Active | keep-refresh | Verify refs. §10. |
| `reference/mcp-servers/README.md` | Active | keep-refresh | Verify 10-server registry + 259-tool count. §10. |
| `reference/mcp-servers/companies.md` | Active | keep-refresh | Verify 44-tool count. §10. |
| `reference/mcp-servers/corpus.md` | Active | keep-refresh | Verify refs. §10. |
| `reference/mcp-servers/portfolio.md` | Active | keep-refresh | Verify refs. §10. |
| `reference/mcp-servers/prediction-markets.md` | Active | keep-refresh | Verify refs. §10. |
| `reference/mcp-servers/scenarios.md` | Active | keep-refresh | Verify refs. §10. |
| `reference/mcp-servers/swarm.md` | Active | keep-refresh | Verify 52-tool count. §10. |
| `reference/ontology-bridge.md` | Active | keep-refresh | Verify refs. §10. |
| `reference/regulation-spans.md` | Active | keep-refresh | Verify refs. §10. |
| `reference/skills/README.md` | Active | keep-refresh | Verify 60-skill count. §10. |
| `reference/upstream-rebase-process.md` | **MISSING** (has prose `**Purpose:**`) | keep-refresh | Add §2 metadata header. `status: Active`. §2 mandatory. |
| `reference/upstream-removal-principles.md` | **MISSING** (has prose `> **Status:**`) | keep-refresh | Add §2 metadata header. `status: Active`. §2 mandatory. |

## New diagrams required (Composition Root gaps)

Per acceptance criterion 7: every crate in the MDS Composition Root gets at
least one diagram. Two crates lack diagrams:

| Crate | Gap | Diagram to generate |
|-------|-----|---------------------|
| `hkask-swarm-widget` | Wired via `hkask-viz-core` (`crates/hkask-viz-core/src/hkask_viz_core.rs:62-63`), renders `swarm` block keyword, but no class diagram exists. | `diagrams/class-hkask-swarm-widget.md` |
| `hkask-event-store` | Wired via `kask_bridge/src/rollout_event_bridge.rs`, consumed by `hkask-regulation/src/cybernetics_loop.rs`, but missing from MDS Composition Root AND no diagram. | `diagrams/class-hkask-event-store.md` + MDS Composition Root update |

## Deletions authorized by §3

| Path | §3 rule | Successor |
|------|---------|-----------|
| `architecture/adr-embedded-yaml-registry.md` | "Superseded → `git rm`; successor carries the content forward" | Body-injection model documented in `architecture/AGENT_SYSTEM_PROMPT.md` and `explanation/skills-and-composition.md`. |

## Verification gate (§10)

After refresh, every file must pass:

- [ ] Six-field metadata header present and correct
- [ ] `mds_categories` field present with ≥1 category
- [ ] Every `##` section has ≥1 footnoted citation with URL (where applicable per type)
- [ ] Every Mermaid block has `DIAGRAM_ALIGNMENT` metadata
- [ ] All internal links resolve
- [ ] No aspirational content in `architecture/`
- [ ] `last_updated` reflects final edit date (2026-08-24)
- [ ] Writing Excellence: ≥3 of 4 perspective tests
- [ ] No stale workspace/project names
