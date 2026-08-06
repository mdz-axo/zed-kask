# Interdisciplinary Constraint-Forces Recast — Document Index

**Status**: Consolidated document base for the `gradient-seeded-recombination` (GSR) and `constraint-forces-recast` (CFR) skills.

**Date**: 2026-08-06

## Document Set

The document base follows the [Diataxis methodology](https://diataxis.fr/) with four quadrants. Each document is classified by its Diataxis quadrant and mapped to [MDS](../architecture/core/MDS.md) categories (domain, composition, trust, lifecycle, curation).

### Explanation (understanding-oriented)

| Document | Purpose | MDS Categories |
|---|---|---|
| [`interdisciplinary-constraint-forces-report.md`](interdisciplinary-constraint-forces-report.md) | **Pass 1 research report.** The provenance table (Phase A), hypothesis set (Phase B), test design (Phase C), gradient map (Phase D), cybernetic loop analysis (Phase E), metacognition log (Phase G), and decoupled critic verdict (Phase H). Explains *why* constraint-forces recasting works, what the falsifier is, and how the Phase F evolution corroborated M1. | domain, trust |
| [`interdisciplinary-constraint-forces-translational-amendment.md`](interdisciplinary-constraint-forces-translational-amendment.md) | **Translational research amendment.** Classifies translational research as the *layer* (directed T-spectrum traversal) and CFR as a *mechanism* within it. Records the Kauffman/Aïra layer-conflation correction and the "terms of art are not common words" trap. Explains *why* the bilingual-mutant variant was a metaphor-driven conflation and why CFR's scope boundary is a boundary, not a bug. | domain, trust |

### Reference (information-oriented)

| Document | Purpose | MDS Categories |
|---|---|---|
| [`interdisciplinary-constraint-forces-frameworks.md`](interdisciplinary-constraint-forces-frameworks.md) | **Pass 2 framework specifications.** GSR (§1) and CFR (§2) as named procedures with inputs, outputs, ontology roles, seed-concept selection rules, convergence criteria, and worked examples. The forcing operator (minimal-satisfiability projection) is specified here. Reference for *what* the frameworks are and *how* to instantiate them. | composition, lifecycle |
| [`interdisciplinary-constraint-forces-skills.md`](interdisciplinary-constraint-forces-skills.md) | **Pass 3 skill family spec.** Manifest sketches, phase coverage, OCAP/gas posture, Phase I deletion-test verdicts, and Phase J skill-discovery gap analysis for both GSR and CFR. Reference for *what* the skills are and *whether* they are essential. | composition, trust, lifecycle |

### How-To (task-oriented)

| Document | Purpose | MDS Categories |
|---|---|---|
| [`enhanced-cleanup-prompt.md`](enhanced-cleanup-prompt.md) | **Enhanced cleanup prompt.** The 12-item cleanup and validation plan with per-task acceptance criteria. How-to for *executing* the cleanup. | lifecycle, curation |
| [`tasks/plan.md`](tasks/plan.md) | **Task plan.** The decomposed task breakdown with phased task list, checkpoints, risks, and open questions. How-to for *tracking* the cleanup. | lifecycle, curation |
| [`tasks/todo.md`](tasks/todo.md) | **Todo checklist.** The flat checklist grouped by phase. How-to for *doing* the cleanup. | lifecycle, curation |

### Tutorial (learning-oriented)

No tutorial document exists yet. A future tutorial could walk a new agent through instantiating CFR on a fresh concept (e.g., GOLEM `Character` → ML-Schema), following the worked example in the frameworks document §2.8. This is a deferred deliverable.

## Scaffolded Skills

The skills are scaffolded as registry crates (manifest.yaml is the reference entity / process to follow; SKILL.md is the descriptor):

| Skill | Process Manifest | Template Directory | SKILL.md |
|---|---|---|---|
| GSR | `kask/registry/manifests/gradient-seeded-recombination.yaml` | `kask/registry/templates/gradient-seeded-recombination/` (7 templates) | `.agents/skills/gradient-seeded-recombination/SKILL.md` |
| CFR | `kask/registry/manifests/constraint-forces-recast.yaml` | `kask/registry/templates/constraint-forces-recast/` (7 templates) | `.agents/skills/constraint-forces-recast/SKILL.md` |

## MDS Category Coverage

| MDS Category | Covered By | Status |
|---|---|---|
| **Domain** | Report (interdisciplinary concept-generation domain), Amendment (translational research layer) | ✅ Covered |
| **Composition** | Frameworks (GSR + CFR composition, delegate skills), Skills (template contracts, span namespaces) | ✅ Covered |
| **Trust** | Report (falsifier, relabel control, three-criterion test), Skills (deletion-test verdicts, gap analysis), Amendment (conflation trap) | ✅ Covered |
| **Lifecycle** | Frameworks (PDCA loops, evolutionary loop), Skills (phase coverage), Tasks (cleanup plan) | ✅ Covered |
| **Curation** | This index, Tasks (plan + todo) | ✅ Covered |

## Key Findings Carried Forward

1. **Forcing operator**: minimal-satisfiability projection (`mutant = argmin_{m ∈ Models(B)} Δ(c, m)`). Not entailment (too strong), not bare satisfiability (too weak). The minimality is load-bearing.
2. **Weakened thesis T1'**: CFR is *a* mechanism for concept generation, not *the* only interdisciplinary operation. Distinct from retrieval-and-grounding (Elicit) and analogy.
3. **Scope boundary**: translational research moves insights into the target vocabulary (translation); CFR keeps the mutant in the source vocabulary (recast). Different operations.
4. **Kauffman A6**: admitted (provenance + admissibility verified via YouTube transcript), empirically untested. Central claim: the biosphere innovates by combinatorial recombination, producing a delay-and-burst trajectory. Falsifier: a domain where the substrate grows but novelty rate does not accelerate.
5. **Ontology sources**: multi-provider abstraction (OBO Foundry primary, OntoBee mirror, BioPortal aggregator). Per-ontology license check is a Guardrail.
6. **Terms of art trap**: "translational" in medicine ≠ "bilingual"; "adjacent possible" in Kauffman ≠ "the next thing over." Surface word association → structural identity claim → framework invention is the conflation shape.

## Validation Status

Both skills have been reviewed through 5 passes (bug-hunt, graph-audit, grill-me, essentialist, and structural verification). Blockers identified and fixed:
- GSR convergence: `hypotenuse` now bound to `1.0 - field_coverage` (was hardcoded `0.0`); `kata_hypotenuse` now reads `step_8_result.hypotenuse` (was reading nonexistent `convergence_metric`); `next_prior_focus` now derived from `priority_ranking[0].location` (was reading nonexistent field).
- CFR convergence: `kata_hypotenuse` now reads `step_8_result` directly (was reading nonexistent `.result`); frontier now carried via `carried_frontier` loop variable (was reading nonexistent `_convergence.frontier_history`); seed index now uses top-level `seed_index` (was using nonexistent `_loop.seed_index`).
- Both skills: `type: object` corrected to `type: array` for collection inputs.
- Both skills: dead RenderAct templates deleted (7/7 surfaces each, ≤7 rule satisfied).
- CFR: `rater` input now consumed in `cfr-three-criterion.j2` procedure body.
- CFR: T1' and "not yet enforced" language added to manifest description.

**Remaining limitations** (recorded honestly):
- `skill-maintenance-validate` not run against R1-R12, Z1-Z8, X1-Z4, E1-E11 (requires the validation tooling).
- Phase F evolution was a single-rater simulation (the author); multi-rater study would strengthen M1 corroboration.
- M3 (shared substrate) not directly tested.
- CFR's forcing operator (minimality) is human-approximated, not mechanically enforced.
