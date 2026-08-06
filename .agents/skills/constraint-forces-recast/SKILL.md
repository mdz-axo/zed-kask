---
name: constraint-forces-recast
description: >
  Core generative process for interdisciplinary concept generation. Recasts
  a seed concept from a source ontology A into a target ontology B's
  constraint context via minimal-satisfiability projection (the mutant is
  the nearest model of B to the seed concept, measured by structural delta).
  Produces a three-criterion mutant that is (i) expressible in A's
  signature, (ii) absent from A, (iii) consistent under B's axioms.
  Generates a relabel control and compares structural deltas — a valid
  mutant must outperform the relabel, discriminating constraint-forces
  from random perturbation. Runs an evolutionary loop over a seed set,
  keeping the Pareto frontier on (novelty, validity, cost-inverted) stable
  for ≥2 iterations. The forcing operator is minimal-satisfiability
  projection. Scope boundary: translational research moves insights into
  the target vocabulary; CFR keeps the mutant in the source vocabulary.
---

# Constraint-Forces Recast

Core generative process for interdisciplinary concept generation. Recasts a seed concept from a source ontology A into a target ontology B's constraint context via **minimal-satisfiability projection** (the mutant is the nearest model of B to the seed concept, measured by structural delta / graph-edit distance). Produces a **three-criterion mutant** that is (i) expressible in A's signature, (ii) absent from A, (iii) consistent under B's axioms.

## When to Use

- When you need to generate a new concept by recasting a seed concept from ontology A into ontology B's constraint context.
- When you have a seed set from `gradient-seeded-recombination` (or a manually-specified seed + target) and want to produce mutants.
- When you want to test whether constraint-forces recasting (M1) produces better mutants than random perturbation (M2) — the relabel control is the discriminating test.

## When NOT to Use

- For finding where to recast — use `gradient-seeded-recombination` (GSR finds sites, CFR recasts).
- For evidence assembly or literature review — use Elicit or `web-deep-research`.
- For translational research (moving insights into the target vocabulary) — CFR keeps the mutant in the source vocabulary; translation moves it to the target. CFR operates within translational steps but does not constitute translation.

## Ontological Anchors

- **Substrate**: Popper falsifiability (the three-criterion test is the falsifier), Platt strong inference (the relabel control is the discriminating test), Pearl counterfactuals (the relabel is the do(not recast) counterfactual), Chamberlin multiple hypotheses (the recast vs relabel is the M1 vs M2 hypothesis pair).
- **Forcing operator**: minimal-satisfiability projection — `mutant(c, A, B) = argmin_{m ∈ Models(B)} Δ(c, m)`, where Δ is graph-edit distance. Not entailment (too strong — projection, not generation) and not bare satisfiability (too weak — doesn't discriminate M1 from M2). The minimality is what forces mutation.
- **Weakened thesis T1'**: constraint-forces recasting is _a_ mechanism for interdisciplinary _concept generation_, distinct from retrieval-and-grounding (evidence assembly, e.g. Elicit) and analogy (communication). Does not claim to be the only interdisciplinary operation.

## PDCA Shape

```
Plan:   Phase 1 — Represent    → Represent seed concept c as an axiom graph
Plan:   Phase 2 — Violate      → Identify B's axioms that c violates
Do:     Phase 3 — Project      → Find minimal-satisfiability projection (the mutant m = argmin Δ(c, m))
Do:     Phase 4 — Control      → Generate relabel control (vocabulary swap, no axiom application)
Check:  Phase 5 — Three-criterion → Check (i) expressible in A's signature, (ii) absent from A, (iii) consistent under B
Check:  Phase 6 — Compare      → Mutant Δ > relabel Δ (else M1 falsified for this cell)
Act:    Phase 7 — Frontier     → Update Pareto frontier on (novelty, validity, cost-inverted)
Check:  Phase 8 — Converge     → Pareto-frontier stability (lisp.eval: hypervolume_delta + 0.05 × new_non_dominated)
Act:    Phase 9 — Loop         → If not converged, re-enter at Phase 1 with next seed from frontier
```

The shape is idiosyncratic to CFR's domain — the Project phase (minimal-satisfiability projection) is the forcing operator, which has no analog in falsifiability (which eliminates, not generates) or gpa-evolution (which evolves text artifacts, not concept graphs).

## Composed Skills

| Skill                           | Role                       | When Invoked                                                                                         |
| ------------------------------- | -------------------------- | ---------------------------------------------------------------------------------------------------- |
| `gradient-seeded-recombination` | Seed selection             | Before CFR — GSR finds recombination sites and selects seed concepts                                 |
| `falsifiability`                | Three-criterion test       | Phase 5 — the three-criterion test is the falsifier                                                  |
| `gpa-evolution`                 | Pareto frontier management | Phase 7 — the frontier update follows gpa-evolution's non-dominated sort + crowding-distance pruning |

## Registry Templates

| Template                 | Type    | Purpose                                                          |
| ------------------------ | ------- | ---------------------------------------------------------------- |
| `cfr-represent.j2`       | KnowAct | Represent seed concept as axiom graph                            |
| `cfr-violate.j2`         | KnowAct | Identify B's axiom violations                                    |
| `cfr-project.j2`         | KnowAct | Minimal-satisfiability projection (the forcing operator)         |
| `cfr-control.j2`         | KnowAct | Generate relabel control (vocabulary swap, no axioms)            |
| `cfr-three-criterion.j2` | KnowAct | Three-criterion test (the falsifier)                             |
| `cfr-compare.j2`         | KnowAct | Compare mutant vs relabel structural delta (discriminating test) |
| `cfr-frontier.j2`        | KnowAct | Update Pareto frontier on (novelty, validity, cost-inverted)     |

## Constraints

- All flow templates are `KnowAct` type with `Public` visibility. Reference documents are `RenderAct`.
- Energy caps: represent (4096), violate (4096), project (6144), control (4096), three-criterion (4096), compare (3072), frontier (3072).
- Gas cap: 150,000 per invocation. Maximum 5 iterations.
- The forcing operator is minimal-satisfiability projection — not entailment, not bare satisfiability. Changing the operator is a breaking change, not a refactor.
- The weakened thesis T1' must not drift back to T1 ("the mechanism"). CFR is _a_ mechanism, not the only one.
- Corroborated is not confirmed. Use "survived", "withstood", "corroborated" — never "proven" or "verified true."
- The relabel control is mandatory — a mutant without a relabel control is not a discriminating test.
- Scope boundary: translational research moves insights into the target vocabulary; CFR keeps the mutant in the source vocabulary. Different operations. Do not add a "bilingual" mode to bridge them — that was a metaphor-driven conflation (see the translational amendment).
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
