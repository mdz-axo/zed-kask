---
name: improve-codebase-architecture
visibility: public
description: "Find deepening opportunities in the codebase. Surfaces shallow modules, proposes deep modules with small interfaces and large implementations. Aligned with hKask's cybernetic design: crate boundaries as first-class seams.
"
---

# Improve Codebase Architecture

Find deepening opportunities in the codebase. Surfaces shallow modules, proposes deep modules with small interfaces and large implementations. Aligned with hKask's cybernetic design: crate boundaries as first-class seams.


## When to Use

- When architectural friction is suspected in a codebase — shallow modules, tight coupling, missing locality, wide import surfaces, or code that is hard to test through its current interface.
- When you need to surface shallow modules and apply the deletion test to determine whether they are pass-throughs or earn their keep.
- When you want to propose deep modules with small interfaces and large implementations, ranked by recommendation strength and explained in terms of leverage and locality.
- When a candidate has been selected and you need to walk the design tree to define the deepened module shape, identify seams and adapters, and confirm the test surface.
- When a deepened design is ready and must be routed to a follow-up action: proceed to refactor, gather more diagnostic data, or defer and record the reasoning.

## Instructions

### arch-explore

1. Walk the codebase organically to find architectural friction — note where you experience difficulty understanding, navigating, or testing rather than applying rigid heuristics.
2. Classify each friction point by signal: understanding one concept requires bouncing between many small modules; interface nearly as complex as the implementation; pure functions extracted for testability while real bugs hide in how they are called; tightly-coupled modules leaking across seams; untestable code through the current interface; wide import surfaces or circular dependencies.
3. Apply the deletion test to suspected shallow modules — if complexity vanishes, the module was a pass-through; if complexity reappears across N callers, it earns its keep.
4. Use the project's domain vocabulary throughout.
5. Flag friction points that contradict known ADRs, but do not recommend revisiting them unless the friction is severe.
6. Do not propose interfaces or refactors — this template is exploration only.

### arch-candidates

1. Assess each friction point and shallow module to propose deepening candidates — refactors that turn shallow modules into deep ones.
2. For each candidate, specify the files involved, the problem (why the current architecture causes friction), the solution (what would change), the benefits, and the recommendation strength (`Strong`, `Worth exploring`, or `Speculative`).
3. Explain every candidate's benefits in terms of locality (how change, bugs, and knowledge concentrate), leverage (how callers benefit from the deeper interface), and testability (how tests would improve).
4. Surface ADR conflicts only when the friction is real enough to warrant revisiting — mark them clearly.
5. Rank the candidates and identify the top recommendation with rationale.
6. Ask the user which candidate to explore.
7. Do not propose interfaces yet — wait for the user to select a candidate.

### arch-deepen

1. Walk the design tree for the selected candidate through each decision in sequence.
2. Define the deepened module shape: specify the public interface items and what complexity the implementation now hides.
3. Design the seam: identify where the interface lives and what adapters satisfy it (production, test, or mock).
4. Confirm the test surface: identify what the module looks like from the outside, which tests survive a refactor, and which tests become easier.
5. Use domain vocabulary in every public interface item — not implementation jargon.
6. Add new glossary terms when naming a deepened module after a concept not yet in the glossary; update the glossary when sharpening a fuzzy term.
7. Propose ADRs sparingly — only when the decision is hard to reverse, surprising without context, and the result of a real trade-off.
8. Offer to record an ADR when the user rejects a candidate with a load-bearing reason that a future explorer would need.

### arch-route

1. Route the deepened design to the correct follow-up based on the decision signal.
2. If proceeding to refactor: recommend `strangler-fig` for incremental migration or `refactor-service-layer` for extracting shared logic, and produce a migration plan with ordered steps and tests that must pass at each step.
3. If more data is needed: recommend the `diagnose` skill, and specify what measurements are needed, how to instrument, and what thresholds would confirm or refute the hypothesis.
4. If deferring or rejecting: produce a decision summary with reasoning, and if a load-bearing reason was given, recommend recording it as an ADR with a suggested title and body.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `arch-explore.j2` | KnowAct | Explore the codebase for architectural friction: shallow modules, tight coupling, missing locality, wide import surfaces, untested code. Apply the deletion test to suspected shallowness.  |
| `arch-candidates.j2` | KnowAct | Present deepening candidates with files, problem, solution, benefits (leverage and locality), and recommendation strength. Use hKask domain vocabulary. Ask user which to explore.  |
| `arch-deepen.j2` | KnowAct | Grill loop for a selected candidate: walk the design tree, define the deepened module shape, identify seams and adapters, confirm test surface. Update glossary and ADRs inline as decisions crystallize.  |
| `arch-route.j2` | KnowAct | Route a deepened architecture design to the appropriate follow-up action (proceed_to_refactor, need_more_data, defer_or_reject).  |

## Fusion Mode

This skill supports **fusion mode** via the `fusion:` block in its flow manifest.
When enabled, all analysis steps route through a multi-model panel — either with
LLM judge synthesis or the **algo / no-judge** path (`judge: algo`) for deterministic
JSON merge without an LLM judge call. This skill uses **deliberation mode** —
Multi-round exploration matches architectural analysis.

The convergence check step has `fusion: false` to ensure deterministic rubric
evaluation uses single-model inference.

## Constraints

- `arch-explore.j2`: Public.
- `arch-candidates.j2`: Public.
- `arch-deepen.j2`: Public.
- `arch-route.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
