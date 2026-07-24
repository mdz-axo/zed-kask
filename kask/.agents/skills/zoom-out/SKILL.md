---
name: zoom-out
visibility: public
description: "Context expansion skill. Zooms out to provide broader architectural context, module maps, caller graphs, data flows, and boundary summaries. Use when lost in implementation details or unfamiliar with a section of the codebase.
"
---

# Zoom Out

Context expansion skill. Zooms out to provide broader architectural context, module maps, caller graphs, data flows, and boundary summaries. Use when lost in implementation details or unfamiliar with a section of the codebase.


## When to Use

- When lost in implementation details and needing broader architectural context.
- When unfamiliar with a section of the codebase and needing a map of the surrounding architecture.
- When needing to evaluate if a generated context map is sufficiently stable for downstream work.
- When uncertainty stems from missing architectural context rather than missing evidence — zooming out provides broader context that raises confidence without gathering more data
- When local confidence is low and the agent suspects the problem is insufficient context scope, not insufficient detail — state uncertainties explicitly with confidence levels and what would resolve them

## Instructions

### zoom-out-context

1. Zoom out from the current focus area to map the surrounding architecture using the project's own domain vocabulary.
2. Produce a module map listing relevant modules, their responsibilities, public interface size, depth, and relationship to the focus area.
3. Trace the caller graph through public interfaces (seams), noting the caller, callee, seam, and direction.
4. Map the data flow, including data types, sources, paths, sinks, and key transformations.
5. Summarize module boundaries relative to the current code, detailing what crosses them, the interface used, and coupling level.
6. Identify key invariants that are not obvious from the code, including how and where they are enforced.
7. Flag areas of uncertainty regarding a module's purpose or behavior, stating confidence levels and what would resolve them.
8. Focus on the current module and its immediate neighbors; do not map the entire system unless explicitly asked.
9. State uncertainties explicitly rather than guessing.

### zoom-out-convergence-check

1. Evaluate the latest context-expansion output to measure convergence on a scale of [0,1].
2. Score the metric where 0 means the context map is fully converged and sufficiently stable for downstream work, and 1 means not converged.
3. Assess saturation to determine how much work remains based on the convergence threshold and iteration context.
4. Return the convergence metric, method, and rationale as JSON.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `zoom-out-context.j2` | KnowAct | Generate a module map, caller graph, data flow summary, boundary summary, and key invariants for the current code region. Uses hKask domain vocabulary.  |
| `zoom-out-convergence-check.j2` | KnowAct | Compute normalized convergence metric for zoom-out PDCA cycles.  |

## Constraints

- `zoom-out-context.j2`: Public.
- `zoom-out-convergence-check.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
