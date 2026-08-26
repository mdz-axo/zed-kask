---
name: constraint-forces-recast
description: "Interdisciplinary concept generation via minimal-satisfiability projection. Recasts a seed concept from ontology A into ontology B's constraint context, producing mutants expressible in A, absent from A, and consistent under B's axioms."
---

# Constraint-Forces Recast

Core generative process for interdisciplinary concept generation. Recasts a seed concept from a source ontology A into a target ontology B's constraint context via **minimal-satisfiability projection** (the mutant is the nearest model of B to the seed concept, measured by structural delta / graph-edit distance). Produces a **three-criterion mutant** that is (i) expressible in A's signature, (ii) absent from A, (iii) consistent under B's axioms.

## When to Use

- When you need to generate a new concept by recasting a seed concept from ontology A into ontology B's constraint context.
- When you have a seed set from `gradient-seeded-recombination` (or a manually-specified seed + target) and want to produce mutants.
- When you want to test whether constraint-forces recasting (M1) produces better mutants than random perturbation (M2) — the relabel control is the discriminating test.

## When NOT to Use

- For finding where to recast — use `gradient-seeded-recombination` (GSR finds sites, CFR recasts).
- For evidence assembly or literature review — use a web-research tool or service (e.g. Elicit); CFR assumes evidence is already gathered.
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
Check:  Phase 8 — Converge     → Pareto-frontier stability (`lisp_eval`: hypervolume_delta + 0.05 × new_non_dominated)
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

| Template | Purpose |
|----------|---------|
| `cfr-represent.j2` | Represent the seed concept c as an axiom graph: nodes are c's structural elements (classes, properties, relations); edges are the axioms connecting them. This is the input to the minimal-satisfiability projection. |
| `cfr-violate.j2` | Identify B's axioms that c violates. For each of B's axioms, check whether c's structure satisfies it. Record the violations. If no violations, c already satisfies B — the minimal-satisfiability projection is the identity (no mutant; the degenerate recast-into-self case). |
| `cfr-project.j2` | Find the minimal structural modification of c that resolves all violations — the mutant m = argmin_{m ∈ Models(B)} Δ(c, m), where Δ is graph-edit distance. The minimality is what forces mutation: if c already satisfies B, no modification (degenerate); if c violates B, the smallest fix is the mutant. This is the forcing operator. |
| `cfr-control.j2` | Generate the relabel control: swap c's vocabulary to B's terms without applying B's axioms (c → B's root class with c's terms relabeled). This is the M2 (random perturbation) control and the do(not recast) counterfactual. |
| `cfr-three-criterion.j2` | Check the three criteria: (i) expressible in A's signature (novel compounds permitted iff compositional), (ii) absent from A (not subsumed by any existing A concept), (iii) consistent under B's axioms. This is the falsifier — a mutant that fails any criterion is not a three-criterion mutant. |
| `cfr-compare.j2` | Compare the mutant's structural delta to the relabel control's structural delta. The mutant must have a larger structural delta than the relabel. If mutant Δ ≤ relabel Δ, the recast did not mutate — it only relabeled. Report falsification of M1 for this cell. |
| `cfr-frontier.j2` | Update the Pareto frontier on (novelty, validity, cost-inverted). Novelty = 1 - (max subsumption by A); validity = fraction of B's axioms satisfied; cost-inverted = 1 - (rater/reasoner effort, normalized). Merge current frontier with newly tested variants, keep non-dominated members, prune by crowding distance if frontier exceeds size limit. |

To render a template, call the `render_template` tool with the template ref (e.g., `essentialist/essentialist-flow`) and a context object with the required variables.

## Constraints

- All flow templates are prompt templates with `Public` visibility. Reference documents are rendering templates.
- The forcing operator is minimal-satisfiability projection — not entailment, not bare satisfiability. Changing the operator is a breaking change, not a refactor.
- The weakened thesis T1' must not drift back to T1 ("the mechanism"). CFR is _a_ mechanism, not the only one.
- Corroborated is not confirmed. Use "survived", "withstood", "corroborated" — never "proven" or "verified true."
- The relabel control is mandatory — a mutant without a relabel control is not a discriminating test.
- Scope boundary: translational research moves insights into the target vocabulary; CFR keeps the mutant in the source vocabulary. Different operations. Do not add a "bilingual" mode to bridge them — that was a metaphor-driven conflation (see the translational amendment).
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
