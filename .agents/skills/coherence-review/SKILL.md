---
name: coherence-review
core: true
visibility: public
description: "Multi-level coherence review. Walks L1-L5 (Boundary, Crate graph, Module, Surface, Code) in order, propagating findings between levels. L1: D-seam integrity via kask-seam-audit. L2: crate graph health via graph-audit semantic mode. L3: module depth via refactor-architecture ra-explore. L4: surface duplication via refactor-architecture ra-audit + kali-audit. L5: code correctness via code-review. Synthesizes a single coherence report with cross-level finding flow. Anchored to kask/docs/architecture/review-reference-models.md."
---

# Coherence Review

Multi-level coherence review for the zed-kask fork. Walks five levels
of abstraction in order — Boundary, Crate graph, Module, Surface, Code
— and synthesizes a single coherence report. The cross-level finding
flow is the core deliverable: an L2 finding ("MCP servers depend on each
other") becomes the scoped input to L3 ("is `hkask-mcp-portfolio`
actually a domain crate?").

Distinct from `constraints-review` (which asks "do the *rules* hang
together?"): `coherence-review` asks "does the *system* hang
together?"

## When to Use

- When you need a coherence snapshot of the kask codebase at multiple
  levels of abstraction — not just code review, not just architecture.
- When a change at one level may have affected another (e.g. a new MCP
  server added L4 surface duplication and L2 layering violations).
- Before a major change (upstream rebase, new crate) to establish a
  coherence baseline.
- When low-level work has been intensive and you suspect the higher
  levels have drifted.

## Reference Models

Anchored to `kask/docs/architecture/review-reference-models.md`. The
five-level structure and per-level convergence signals are justified by:

- **Kruchten 4+1** — multiple views per stakeholder justifies the five
  levels. The "+1" (scenarios) maps to L5.
- **SEI ATAM** — quality-attribute scenarios justify the per-level
  convergence signals.
- **Simon near-decomposability** — justifies the L1 boundary and L2
  crate-graph layering.
- **Courtois** — formalizes the L2 crate-graph analysis.
- **Ashby requisite variety** — justifies the per-level check sets (each
  level needs enough distinct checks to catch its failure modes).
- **Murphy reflexion models** — justifies the IS/OUGHT structure:
  DIVERGENCE.md is the intended model, the crate graph is the actual,
  the cross-level finding flow is the reflexion. (⚠️ Murphy citation is
  inferred — see the calibration doc.)

## The Five Levels

| Level | View (4+1) | Quality attribute (ATAM) | Question | Tool | Convergence signal |
|---|---|---|---|---|---|
| L1 Boundary | Physical/Deployment | Modifiability (upstream sync cost) | Is the kask/upstream boundary clean and complete? | `kask-seam-audit` | Every `// zed-kask:` comment has a D-seam + test |
| L2 Crate graph | Development (module) | Modifiability + maintainability | Does dependency direction make sense? | `graph-audit` semantic mode | Graph-health metric < 0.15; zero cycles; zero surface-to-surface deps |
| L3 Module | Logical | Modifiability + testability | Is each crate a deep module? | `refactor-architecture` ra-explore + ra-candidates | Deletion-test verdict per crate |
| L4 Surface | Process (interaction) | Modifiability + consistency | Are surfaces non-duplicated and thin? | `refactor-architecture` ra-audit + `kali-audit` | Zero Identical/Divergent duplication |
| L5 Code | Scenarios (+1) | Correctness + security | Does this code do what it says, safely? | `code-review` | Review verdict + test pass |

## Instructions

### cr-l1-boundary

1. Run `kask-seam-audit` on the DIVERGENCE.md D-seam surface.
2. Check: every `// zed-kask:` comment in `crates/` has a corresponding
   D-seam entry. Every D-seam entry has a pinning test.
3. Check D-seam numbering gaps (e.g. D17, D19) — are they retired or
   missing?
4. Produce L1 findings: boundary violations, missing tests, numbering
   gaps. These scope the L2 review (a missing D-seam means the crate
   graph may have an undocumented upstream touch).

### cr-l2-crate-graph

1. Run `graph-audit` in semantic mode on the kask crate graph (18 core
   crates + 13 MCP servers).
2. Check: zero dependency cycles, zero surface-to-surface deps (MCP
   server depending on another MCP server), no god-crates (fan-in > 10),
   layering holds (storage → domain → MCP base → MCP servers → bridge).
3. Produce L2 findings: layering violations, god-crates, missing leaf
   crates. These scope the L3 review (a god-crate or a server-as-domain
   gets the deletion test).

### cr-l3-module

1. For each crate flagged by L2 (or a user-specified subset), run
   `refactor-architecture` `ra-explore` + `ra-candidates`.
2. Apply the deletion test: if you deleted this crate, would complexity
   vanish or reappear across N callers?
3. Check interface size (the 7-function rule from `deep-module`).
4. Produce L3 findings: shallow modules, wide interfaces, trait-with-one-impl.
   These scope the L4 review (a crate doing multiple jobs may have
   surface duplication).

### cr-l4-surface

1. Run `refactor-architecture` `ra-audit` across MCP/CLI/API surfaces.
2. Classify duplicated operations: Identical, Divergent, Surface-only,
   Pass-through.
3. Run `kali-audit` on MCP tool surfaces for security thinness.
4. Produce L4 findings: duplicated logic, thick adapters, ocap declared
   but not wired. These scope the L5 review (duplicated logic is where
   bugs hide).

### cr-l5-code

1. For each surface/crate flagged by L4 (or a user-specified subset),
   run `code-review`.
2. Check: correctness, silent-error patterns (`unwrap()`, `let _ =`,
   `unwrap_or(0)` on sense inputs), test pass.
3. Produce L5 findings: bugs, silent failures, missing tests.

### cr-synthesize

1. Aggregate findings across L1–L5.
2. Make the cross-level finding flow explicit: which L(n) finding scoped
   which L(n+1) review? Which findings propagated to multiple levels?
3. Produce a single coherence report: per-level findings, cross-level
   flow, overall coherence verdict.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `cr-l1-boundary.j2` | `KnowAct` | L1: D-seam integrity via kask-seam-audit. |
| `cr-l2-crate-graph.j2` | `KnowAct` | L2: crate graph health via graph-audit semantic mode. |
| `cr-l3-module.j2` | `KnowAct` | L3: module depth via refactor-architecture ra-explore. |
| `cr-l4-surface.j2` | `KnowAct` | L4: surface duplication via ra-audit + kali-audit. |
| `cr-l5-code.j2` | `KnowAct` | L5: code correctness via code-review. |
| `cr-synthesize.j2` | `KnowAct` | Synthesize cross-level coherence report. |

## Constraints

- All templates are `KnowAct` type with `Public` visibility.
- Levels are ordered L1→L5. Do not skip levels. An L(n) finding scopes
  the L(n+1) review — running L5 without L2 is a category error.
- The synthesis step is what makes this a skill, not a router. The
  cross-level finding flow is the deliverable.
- Registry is authoritative — when this SKILL.md disagrees with registry
  templates, the registry wins.
