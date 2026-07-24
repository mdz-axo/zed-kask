---
name: pragmatic-laziness
visibility: public
description: "Procedural composition skill that finds the path of least action through meaning-space. Describes the intersection of pragmatic-semantics, pragmatic-cybernetics, essentialist, and grill-me through a 3-phase lazy loop.
"
---

# Pragmatic Laziness

Procedural composition skill that finds the path of least action through meaning-space. Describes the intersection of pragmatic-semantics, pragmatic-cybernetics, essentialist, and grill-me through a 3-phase lazy loop.


## When to Use

- When you need to find the path of least action through meaning-space by decomposing a situation into syntax, semantics, and pragmatics layers.
- When you need to identify feedback loops (OODA, cybernetic, REPL) and locate effort hotspots where energy expenditure outweighs value produced.
- When you need to apply the deletion test and brachistochrone rule to eliminate unnecessary complexity and find the least-action configuration.
- When you need to verify if a system configuration has reached a stationary point (δS = 0) where no further action reduction is possible.

## Instructions

### lazy-decompose

1. Separate the situation into three layers using Morris's Triad of semiotics.
2. Extract structural elements, dependencies, call graphs, and module boundaries into the syntax layer.
3. Extract types, contracts, invariants, data flow, and literal behavior into the semantics layer.
4. Extract context-dependent intent, enablements, and user needs into the pragmatics layer.
5. Classify each element as structural (syntax), definitional (semantics), or contextual (pragmatics).
6. Assign an eliminability rating to pragmatic elements, as they are the primary candidates for Phase 3 reduction.

### lazy-identify-loops

1. Map the feedback loops operating in the system, including OODA, cybernetic, and REPL loops.
2. Check each loop for closure, fidelity, gain, and delay.
3. Locate effort hotspots where the most energy is being expended per unit of value produced.
4. Map each effort hotspot to an element from the decomposed layers.
5. Flag broken loops prominently, as they represent wasted effort and prime reduction candidates.

### lazy-stationary-action

1. Apply the deletion test to every element identified in Phase 1.
2. Apply the brachistochrone check to determine if apparent complexity actually reduces total system action elsewhere.
3. Mentally delete each candidate reduction and assess what breaks or becomes harder.
4. Eliminate elements if nothing breaks, or if the fix is trivial and can be inlined.
5. Retain elements if deletion increases total system action.
6. Prioritize elements flagged as effort hotspots and classified as pragmatic with high eliminability.

### pragmatic-laziness-converge

1. Determine whether the system has reached stationary action (δS = 0) where no further reduction in total system action is possible.
2. Check if the element count, structure, behavior, or effort distribution has changed between configurations.
3. Return a stationary verdict if no elements were eliminated and no structure or behavior changed.
4. Continue the loop if elements were eliminated or structure was simplified.
5. Revert and escalate if elements were added or structure was complicated.
6. Escalate to human if maximum iterations are reached without convergence.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `lazy-decompose.j2` | KnowAct | Phase 1 of the 3-phase lazy loop. Decompose the situation into syntax/semantics/pragmatics layers using Morris's Triad.  |
| `lazy-identify-loops.j2` | KnowAct | Phase 2 of the 3-phase lazy loop. Identify feedback loops (OODA, cybernetic, REPL) and effort distribution from the decomposed layers produced by Phase 1.  |
| `lazy-stationary-action.j2` | KnowAct | Phase 3 of the 3-phase lazy loop. Apply the deletion test and brachistochrone rule to find the least-action configuration. Consumes outputs from Phase 1 (decomposition) and Phase 2 (loops).  |
| `pragmatic-laziness-converge.j2` | KnowAct | δS = 0 convergence check. Compares current and previous configurations to decide whether the lazy loop has reached a stationary point.  |

,## Fusion Mode

This skill supports **fusion mode** via the `fusion:` block in its flow manifest.
When enabled, all analysis steps route through a multi-model panel — either with
LLM judge synthesis or the **algo / no-judge** path (`judge: algo`) for deterministic
JSON merge without an LLM judge call. Pragmatic-laziness uses **synthesis mode**
(compose best elements) to match least-action pathfinding.

The convergence check step has `fusion: false` to ensure deterministic rubric
evaluation uses single-model inference.

## Constraints

- `lazy-decompose.j2`: Public.
- `lazy-identify-loops.j2`: Public.
- `lazy-stationary-action.j2`: Public.
- `pragmatic-laziness-converge.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
