---
name: gradient-seeded-recombination
description: "Find where to apply constraint-forces recast. Inventories ontology namespaces, builds a complete-graph prior, maps the recombination field, detects gradients, generates reason hypotheses, and selects seed concepts."
---

# Gradient-Seeded Recombination

Substrate-analysis process for interdisciplinary concept generation. Finds **where** to apply constraint-forces recast — inventories ontologies, maps recombination gradients between them, prioritizes sites, and selects seed concepts. Does not itself recast concepts (that is `constraint-forces-recast`'s job).

## When to Use

- When you need to find cross-ontology recombination sites — pairs of ontologies where a concept from A should be recast into B's constraint context but hasn't been.
- When you need a reusable gradient map over an ontology registry, consumed by CFR or by a future ontology-coverage-audit.
- When you want to extend the project's 6 domain-supplement namespaces with external ontologies (OBO Foundry, OntoBee, BioPortal) with per-ontology license checking.

## When NOT to Use

- For gradient analysis within a codebase or telemetry field — use `gradient-hunter` (different substrate).
- For the actual concept recast — use `constraint-forces-recast` (GSR finds sites, CFR recasts).
- For evidence assembly or literature review — use Elicit or `web-deep-research`.

## Ontological Anchors

- **Substrate**: Parisi spin glass theory (non-ergodicity, frustration, metastable valleys) — the same substrate as gradient-hunter, applied to cross-ontology recombination sites.
- **Surface**: the gradient-hunter eight-shape taxonomy (sharp cliff, roof edge, wombling boundary, regression discontinuity, topological hole, oracle gap, frustrated landscape, allosteric population shift) + the seven-class reason taxonomy (Rubin MCAR/MAR/MNAR + spin glass metastable trap + allostery broken coupling).
- **Domain supplement**: the project's 6 domain-supplement ontology namespaces (FIBO, ESO, GOLEM, ML-Schema, OMC, SUMO) + 2 universal axes (PKO, DC+BIBO) + 5W1H core, extended via OBO Foundry / OntoBee / BioPortal.

## PDCA Shape

```
Plan:   Phase 1 — Inventory    → Enumerate ontology namespaces + key concepts
Plan:   Phase 2 — Prior        → Build complete-graph prior K_n
Do:     Phase 3 — Map          → Map actual recombination field
Do:     Phase 4 — Detect       → Classify gradients by 8-shape taxonomy + fractal recurrence
Check:  Phase 5 — Hypothesize  → Generate reason hypotheses (7-class taxonomy)
Check:  Phase 6 — Prioritize   → Rank sites by reason class > fractal recurrence > magnitude
Act:    Phase 7 — Select seeds → Pick most-central concept per high-priority site
Check:  Phase 8 — Converge     → Cauchy criterion on gradient map stability
Act:    Phase 9 — Loop         → If not converged, re-enter at Phase 2 with refined prior
```

The shape is idiosyncratic to GSR's domain — the Inventory phase (enumerate ontologies) and Select-Seeds phase (pick most-central concept) have no analog in gradient-hunter, because the substrate is ontology namespaces, not code artifacts.

## Composed Skills

| Skill                     | Role                          | When Invoked                                                     |
| ------------------------- | ----------------------------- | ---------------------------------------------------------------- |
| `pragmatic-cybernetics`   | Prior modeling                | Prior phase — when no sibling or convention prior is available   |
| `graph-audit` (code mode) | Field topology extraction     | Map phase — when hunting topology gradients                      |
| `falsifiability`          | Counterfactual discrimination | Hypothesize phase — discriminating between reason hypotheses     |
| `metacognition`           | Prior perspective rotation    | Hypothesize phase — different priors surface different gradients |

## Registry Templates

| Template              | Type    | Purpose                                                                     |
| --------------------- | ------- | --------------------------------------------------------------------------- |
| `gsr-inventory.j2`    | KnowAct | Enumerate ontology namespaces, fetch external ontologies with license check |
| `gsr-prior.j2`        | KnowAct | Build complete-graph prior K_n                                              |
| `gsr-map.j2`          | KnowAct | Map actual recombination field                                              |
| `gsr-detect.j2`       | KnowAct | Detect gradients, classify by 8-shape taxonomy, fractal recurrence check    |
| `gsr-hypothesize.j2`  | KnowAct | Generate reason hypotheses via 7-class taxonomy                             |
| `gsr-prioritize.j2`   | KnowAct | Prioritize sites by reason class > fractal recurrence > magnitude           |
| `gsr-select-seeds.j2` | KnowAct | Select most-central concept per high-priority site                          |

## Constraints

- All flow templates are `KnowAct` type with `Public` visibility. Reference documents are `RenderAct`.
- rJoule cap: 2 per invocation. Maximum 3 iterations.
- The prior must be explicit before gradient detection.
- The fractal recurrence check is mandatory.
- Per-ontology license check is a Guardrail — do not assume blanket CC-BY (OBO Foundry licenses span CC-BY 4.0, CC-BY 3.0, CC0, Apache 2.0, GPL-3.0, Artistic-2.0).
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
