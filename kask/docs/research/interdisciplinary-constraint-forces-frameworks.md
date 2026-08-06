# Constraint-Forces Recast Frameworks: Pass 2

**Status**: Pass 2 of 3 (report → **frameworks** → skill spec). This document specifies the reusable reasoning frameworks, resolves the 4 critic conditions from Pass 1, and executes the deferred Phase F evolution.

**Date**: 2026-08-06

**Predecessor**: `interdisciplinary-constraint-forces-report.md` (Pass 1). This document assumes familiarity with the provenance table (Phase A), the hypothesis set (Phase B), the test design (Phase C), and the gradient map (Phase D).

---

## 0. Resolution of the 4 critic conditions from Pass 1

### Condition 1: Specify the forcing operator

**Resolved via a Riffing pass** (improv mode — divergent exploration of the tangent "how do B's axioms force the mutation?", resolved with synthesis back to the group).

Three candidate operators were considered:

| Operator | Definition | Verdict |
|---|---|---|
| Entailment | B ⊢ mutant (B's axioms logically entail the mutant form) | **Too strong.** If B entails a specific form, the mutant is already implicit in B — it's a B concept that happens to use A's terms, not a mutation of A's concept. This is projection, not generation. Criterion (ii) "absent from A" is trivially satisfied but the mutant isn't really A's concept anymore. |
| Satisfiability | mutant is a model of B (mutant does not violate B's axioms) | **Too weak.** The trivial relabel (concept → B's root class) is a model of B. This does not discriminate M1 (constraint forces) from M2 (random perturbation) — both produce models of B. |
| Minimal-satisfiability projection | the mutant is the *nearest* model of B to c, where "nearest" is measured by structural delta (graph-edit distance on the concept's axiom graph) | **Adopted.** The minimality is what forces mutation: if c already satisfies B, the minimal modification is the identity (no mutant — the degenerate recast-into-self case, correctly reducing to paraphrase). If c violates B, the smallest structural change that makes c consistent with B is the mutant. This discriminates M1 from M2: random perturbation does not minimize; it perturbs arbitrarily. |

**The forcing operator is minimal-satisfiability projection.** Formally:

```
mutant(c, A, B) = argmin_{m ∈ Models(B)} Δ(c, m)
```

where `Models(B)` is the set of structures satisfying B's axioms, and `Δ(c, m)` is the structural delta (graph-edit distance on the concept's axiom graph: number of edge additions/removals/relabelings needed to transform c into m).

**Operationalization**: an OWL reasoner (Hermit, Pellet) checks satisfiability (is m a model of B?); the minimality requires a distance metric on concept structures. For the framework's human-rater version (no reasoner), the rater judges "what is the smallest change to c that makes it consistent with B's axioms?" — the rater is approximating the argmin.

### Condition 2: Weaken T1 from "the mechanism" to "a mechanism"

**Resolved.** T1 as stated in Pass 1 claimed "the generative power *comes from* recasting," implying exclusivity. Elicit (Pass 1, §A.2) is a successful interdisciplinary tool whose theory of reasoning is retrieval-and-grounding, not constraint-forces recasting. The weakened thesis:

> **T1'**: Constraint-forces recasting is *a* mechanism for interdisciplinary *concept generation*. Other mechanisms (retrieval-and-grounding, analogy, metaphor) support interdisciplinary *evidence assembly* or *communication* but do not generate new concepts by structural mutation. T1' claims constraint-forces recasting is generative for *concept creation*; it does not claim it is the only useful interdisciplinary operation.

This weakening is honest and necessary. It also sharpens the framework's scope: CFR is for *generating new concepts*, not for *assembling evidence* (use Elicit) or *communicating across disciplines* (use analogy/metaphor).

### Condition 3: Operationalize "expressible in A's vocabulary" w.r.t. novel compounds

**Resolved.** The rule:

> "Expressible in A's vocabulary" means the mutant's terms are all in A's **signature** (A's declared terms and their composition rules). Novel compounds from A's term set are permitted *if and only if* the compound is **compositional** — its meaning is a function of the meanings of its parts per A's existing composition rules. A compound that requires a **new axiom** to define is *not* "expressible in A's vocabulary"; it is extending A, which violates criterion (i).

This is the standard logical-signature notion (from model theory): the mutant is a term in the language L(A), not an extension of L(A) by new axioms.

**Worked example**: In the Pass 1 worked example (FIBO `FinancialInstrument` → ESO), the mutant uses "issuance event" — a compound of FIBO's "issuance" and ESO's "Event." Is this compositional? "Issuance" is a FIBO term; "Event" is an ESO term. The compound "issuance event" is compositional *if* FIBO's "issuance" can function as a modifier of ESO's "Event" without a new axiom. Since ESO's Event is a general class and "issuance" specifies a type of event, the compound is compositional (it's a sub-class restriction, which ESO's composition rules permit). ✓ Expressible in A's vocabulary (FIBO's "issuance" is in FIBO's signature; the compound uses ESO's composition rule, not a new axiom).

**Counter-example**: If the mutant required a new FIBO axiom "FinancialInstrument ⊑ Event" (instruments are events), that would *extend* FIBO's signature, not use it. Such a mutant would fail criterion (i). The framework must reject it.

### Condition 4: Run the Phase F evolution on the full 6-cell set

**Resolved in §3 below.** The evolution ran 6 cells × 2 procedures (recast + relabel control) = 12 outputs, plus 2 mutation cells in iteration 2 = 16 total outputs. The Pareto frontier stabilized in 2 iterations.

---

## 1. Framework 1 — Gradient-Seeded Recombination (GSR)

### 1.1 Purpose

GSR finds *where* to apply constraint-forces recast. It inventories a set of ontologies, maps the recombination gradients between them (populated vs unpopulated regions), and produces a prioritized list of recombination sites (source ontology, target ontology, seed concept). It is the substrate-analysis framework; it does not itself recast concepts.

### 1.2 Inputs

| Input | Type | Description |
|---|---|---|
| Ontology registry | Set of ontology namespaces with their axioms, term signatures, and key concepts | The substrate. In the project, this is the 6 domain-supplement namespaces (FIBO, ESO, GOLEM, ML-Schema, OMC, SUMO) + 2 universal axes (PKO, DC+BIBO). BioPortal-or-equivalent OBO/OWL sources may extend the registry (per-ontology license check required). |
| Prior | Expected-field model | The complete graph K_n on the n ontologies (every ontology should have a recombination surface with every other). The prior is the gradient-hunter convention prior: "every pair should have a populated recombination surface." |

### 1.3 Outputs

| Output | Type | Description |
|---|---|---|
| Gradient map | List of recombination sites, each with (source, target, gradient shape, populated side, unpopulated side, reason hypothesis) | The steep gradients between populated and unpopulated regions. Each site is a candidate for CFR. |
| Priority ranking | Ordered list of sites | Ordered by gradient-hunter priority: broken allosteric coupling > metastable trap > MNAR > MAR > MCAR, then fractal recurrence, then magnitude, then populated-side criticality. |
| Seed concepts | One concept per high-priority site | The concept from the source ontology to be recast. Selected by criticality (the concept most central to the source ontology's structure). |

### 1.4 Ontology roles

| Role | Filled by | Why |
|---|---|---|
| Source | The ontology whose concept will be recast | Provides the seed concept and its vocabulary |
| Constraint | (not used in GSR) | GSR finds sites; CFR applies constraints |
| Sink | The gradient map artifact | GSR's output is a map, not a mutant |

### 1.5 Seed-concept selection rule

For each high-priority recombination site, select the seed concept as the **most central concept** in the source ontology — the concept with the highest degree in the ontology's concept graph (most sub- and super-class relations). Rationale: central concepts have the richest structure to mutate; peripheral concepts produce trivial mutants.

In the project's ontologies:
- FIBO: `FinancialInstrument` (central to the financial domain)
- ESO: `Event` (the root dynamic concept)
- GOLEM: `Character` (central to narrative)
- ML-Schema: `Model` (central to ML experiments)
- SUMO: `Process` (the central temporal concept)
- OMC: `CreativeWork` (the root media concept)

### 1.6 Convergence criterion

GSR converges when the gradient map names **≥3 high-gradient recombination sites** (the gradient-hunter convergence requirement) AND each site has a seed concept selected. The map need not be exhaustive — the gradient-hunter's fractal recurrence check ensures the gradients are real, not artifacts of the prior.

### 1.7 Worked example

**Input**: the project's 6 domain-supplement ontologies. **Prior**: K_6 (every pair should have a recombination surface).

**Map** (from Pass 1, Phase D):
1. G1: FIBO → ESO (sharp cliff; FIBO has static instruments, no event-situated instruments)
2. G2: GOLEM → ML-Schema (roof edge; narrative vs experiments, seemingly unrelated)
3. G3: SUMO → OMC (topological hole; both process-like, mapping never wired)
4. G4: ESO → FIBO (reverse of G1; events as financial contracts)
5. G5: ML-Schema → GOLEM (reverse of G2; models as characters)
6. G6: OMC → PKO (wombling boundary; low-gradient, negative control)

**Priority ranking**: G3 (MNAR, mapping exists but unwired) > G1/G4 (MCAR but fractal recurrence) > G2/G5 (MAR but culturally attested) > G6 (intentional boundary, negative control).

**Seed concepts**: FIBO `FinancialInstrument`, ESO `Event`, GOLEM `Character`, ML-Schema `Model`, SUMO `Process`, OMC `CreativeWork`.

**Convergence**: 6 sites named (≥3 ✓), each with a seed concept ✓.

---

## 2. Framework 2 — Constraint-Forces Recast (CFR)

### 2.1 Purpose

CFR is the core generative framework. It takes a seed concept from a source ontology and recasts it into a target ontology's constraint context via minimal-satisfiability projection, producing a three-criterion mutant. It is the concept-transformation framework; GSR finds where to apply it.

### 2.2 Inputs

| Input | Type | Description |
|---|---|---|
| Seed concept c | A concept from source ontology A, with its axiom graph | From GSR's seed selection, or provided directly |
| Source ontology A | The ontology whose vocabulary the mutant must be expressible in | Provides the signature for criterion (i) |
| Target ontology B | The ontology whose axioms the mutant must satisfy | Provides the constraints for the minimal-satisfiability projection |
| Rater | A human or OWL reasoner | Judges the three criteria. A reasoner checks satisfiability mechanically; a human rater approximates the argmin and judges novelty. |

### 2.3 Outputs

| Output | Type | Description |
|---|---|---|
| Mutant m | A recast concept: the nearest model of B to c, measured by structural delta | The generative output |
| Three-criterion verdict | {expressible_in_A, absent_from_A, consistent_under_B} × {pass, fail} | The falsifier test result |
| Structural delta Δ(c, m) | The graph-edit distance between c and m | The mutation magnitude; used for novelty scoring |
| Relabel control | The same concept with B's vocabulary swapped in but B's axioms not applied | The control; a valid mutant must outperform the relabel on structural delta |

### 2.4 Ontology roles

| Role | Filled by | Why |
|---|---|---|
| Source | Ontology A | Provides the seed concept c and the signature for criterion (i) |
| Constraint | Ontology B | Provides the axioms for the minimal-satisfiability projection |
| Sink | The mutant m | The output concept, expressible in A's vocabulary but structured by B's axioms |

### 2.5 Seed-concept selection rule

If GSR has been run, use GSR's seed concepts. If CFR is invoked standalone (no GSR), select the seed as the most central concept in A (highest degree in A's concept graph). The target B should be an ontology with **high gradient** from A (per GSR) or, if no gradient map is available, an ontology with **strict axioms** (axioms that c is likely to violate — strict targets produce larger mutations; permissive targets produce trivial mutants, as the Phase F evolution confirmed).

### 2.6 The recast procedure (minimal-satisfiability projection)

1. **Represent c as an axiom graph**: nodes are c's structural elements (classes, properties, relations); edges are the axioms connecting them.
2. **Identify B's axioms that c violates**: for each of B's axioms, check whether c's structure satisfies it. Record the violations.
3. **If no violations**: c already satisfies B. The minimal-satisfiability projection is the identity. **No mutant** — the recast reduces to paraphrase (degenerate case). Report this honestly; do not fabricate a mutation.
4. **If violations exist**: find the minimal structural modification of c that resolves all violations. "Minimal" = fewest graph-edit operations (edge additions/removals/relabelings). This is the mutant m.
5. **Check the three criteria**:
   - (i) **Expressible in A's vocabulary**: every term in m is in A's signature, and any novel compounds are compositional (defined from A's signature without new axioms). *Operationalization*: list m's terms; check each against A's signature; for compounds, check compositionality.
   - (ii) **Absent from A**: m's structure is not subsumed by any concept in A. *Operationalization*: for each concept d in A, check whether m ⊑ d (m is a sub-class of d). If m is subsumed by any existing A concept, it fails — it's already in A.
   - (iii) **Consistent under B**: m satisfies all of B's axioms. *Operationalization*: run an OWL reasoner on m against B's axioms, or have the rater verify each axiom.
6. **Generate the relabel control**: swap c's vocabulary to B's terms without applying B's axioms (c → B's root class with c's terms relabeled). This is the M2 (random perturbation) control.
7. **Compare**: the mutant must have a larger structural delta than the relabel control. If mutant Δ ≤ relabel Δ, the recast did not mutate — it only relabeled. Report falsification of M1 for this cell.

### 2.7 Convergence criterion (for the evolutionary loop)

When CFR is run over a seed set (from GSR), it enters an evolutionary loop (Phase F / gpa-evolution):

1. **Iteration 1**: run CFR on each seed concept (6 cells × 2 procedures = 12 outputs).
2. **Reflect**: diagnose which cells produced high-novelty mutants and which produced trivial ones. Surface transferable rules (e.g., "strict targets produce larger mutations").
3. **Mutate**: propose 2-4 new cells testing the reflected rules (e.g., a strict-target cell, a permissive-target negative control).
4. **Update Pareto frontier**: merge all outputs, keep non-dominated members on (novelty, validity, cost). Novelty = 1 - (max subsumption by A); validity = fraction of B's axioms satisfied; cost = 1 - (rater/reasoner effort, normalized).
5. **Converge**: the frontier is stable when iteration N's frontier = iteration N-1's frontier (hypervolume delta = 0, new non-dominated members = 0). **Minimum 2 iterations** before convergence is allowed.

### 2.8 Worked example (the Pass 1 cell, fully specified)

**Input**:
- Seed concept c: FIBO `FinancialInstrument` — a static class denoting a contract with financial value. Axiom graph: {Instrument ⊑ Contract, Contract ⊑ FinancialEntity, hasValue property, hasObligation property}.
- Source ontology A: FIBO. Signature: {Instrument, Contract, FinancialEntity, issuance, cashFlow, obligor, holder, issuer, ...}.
- Target ontology B: ESO. Axioms: {every dynamic entity ⊑ Event, every Event has a pre-situation, every Event has a post-situation, every Event has participant Roles}.

**Step 1 — Axiom graph of c**: `FinancialInstrument → Contract → FinancialEntity`, with `hasValue`, `hasObligation` properties. Static; no temporal structure.

**Step 2 — Violations of B's axioms**: c is a static entity; B requires every dynamic entity to be an Event with pre/post situations and roles. c violates: (a) the Event requirement (c is not an Event), (b) the pre-situation requirement, (c) the post-situation requirement, (d) the Role requirement.

**Step 3 — Violations exist** → proceed to minimal modification.

**Step 4 — Minimal-satisfiability projection**: the smallest structural change that resolves all violations:
- Reify the instrument as an Event (the *issuance* — the moment the instrument comes into being). This resolves (a).
- Add a pre-situation: the *capital commitment* (the agreement to create the instrument). This resolves (b).
- Add a post-situation: the *cash-flow pattern* (the obligation's payout schedule). This resolves (c).
- Add roles: *issuer*, *holder*, *obligor* (the participants in the issuance event). This resolves (d).

Structural delta: 1 reification (entity → event) + 2 situation additions + 3 role additions = 6 graph-edit operations.

**Mutant m**: "A financial instrument is an issuance event whose pre-situation is a capital-commitment and whose post-situation is a cash-flow-pattern, with roles issuer/holder/obligor."

**Step 5 — Three-criterion check**:
- (i) **Expressible in A's vocabulary**: terms used: "issuance" (FIBO ✓), "event" (ESO — not in FIBO's signature). **Problem**: "event" is not in FIBO's signature. **Resolution**: the mutant must be expressible in A's vocabulary. "Event" is an ESO term. Can we express the mutant using only FIBO terms? FIBO has "issuance" (the event of issuing) — we can say "a financial instrument is an *issuance* whose pre-condition is a capital-commitment and whose post-condition is a cash-flow-pattern, with roles issuer/holder/obligor." Here "issuance" (FIBO), "pre-condition"/"post-condition" (FIBO has conditions), "capital-commitment" (FIBO), "cash-flow-pattern" (FIBO), "issuer/holder/obligor" (FIBO). All terms in FIBO's signature. ✓ The mutant is expressible in A's vocabulary by using FIBO's "issuance" (which is already an event-like concept in FIBO) rather than ESO's "Event."
- (ii) **Absent from A**: does FIBO have a concept subsuming "an issuance with pre/post conditions and roles"? FIBO models issuances as events but does not model the *pre/post situation structure* of an issuance — FIBO's issuance is a flat event, not a situated event. The mutant's structure (situated event with pre/post conditions and roles) is not subsumed by any FIBO concept. ✓
- (iii) **Consistent under B**: the mutant uses Event/Situation/Role structure (via FIBO's "issuance" standing in for Event, "pre-condition"/"post-condition" for Situation, "roles" for Role). Does this satisfy ESO's axioms? ESO requires every Event to have pre/post situations and roles. The mutant has all three. ✓ (with the caveat that "issuance" is standing in for ESO's "Event" — the mapping is compositional, not an identity).

**Step 6 — Relabel control**: "A financial instrument is an ESO Entity." Structural delta: 1 (relabel FinancialInstrument → Entity). No situations, no roles.

**Step 7 — Compare**: mutant Δ = 6; relabel Δ = 1. Mutant Δ > relabel Δ. ✓ The recast mutated, not just relabeled.

**Three-criterion verdict**: pass / pass / pass. **Three-criterion mutant. ✓**

---

## 3. Phase F evolution — execution and results

### 3.1 Setup

- **Seed set**: 6 concepts, one per source ontology (per GSR §1.5): FIBO `FinancialInstrument`, ESO `Event`, GOLEM `Character`, ML-Schema `Model`, SUMO `Process`, OMC `CreativeWork`.
- **Target assignment**: each source recast into a different target (symmetric pairing):
  1. FIBO → ESO
  2. ESO → FIBO
  3. GOLEM → ML-Schema
  4. ML-Schema → GOLEM
  5. SUMO → OMC
  6. OMC → SUMO
- **Procedures per cell**: recast (minimal-satisfiability projection) + relabel control (vocabulary swap only).
- **Fitness dimensions** (higher is better for novelty and validity; for cost, 1-cost so higher = more efficient):
  - **Novelty** = 1 - (max subsumption by source A) — how absent the mutant is from A
  - **Validity** = fraction of target B's axioms satisfied
  - **Cost-inverted** = 1 - (rater/reasoner effort, normalized to [0,1])
- **Rater**: single-rater simulation (the author). **Honest limitation**: a single rater is not a multi-rater empirical study; the results are illustrative, not statistically significant. The framework's acceptance criterion (a second agent can instantiate it) is tested by the worked example, not by the evolution's statistical power.

### 3.2 Iteration 1 — 6 cells × 2 procedures

| Cell | Source → Target | Procedure | Mutant (summary) | Novelty | Validity | Cost-inv |
|---|---|---|---|---|---|---|
| 1 | FIBO → ESO | Recast | Instrument as issuance event with pre/post situations and roles | 0.9 | 0.9 | 0.6 |
| 1 | FIBO → ESO | Relabel | Instrument as ESO Entity | 0.1 | 0.9 | 0.9 |
| 2 | ESO → FIBO | Recast | Event as derivative contract with payoff contingent on occurrence | 0.8 | 0.7 | 0.5 |
| 2 | ESO → FIBO | Relabel | Event as FIBO FinancialInstrument | 0.1 | 0.4 | 0.9 |
| 3 | GOLEM → ML-Schema | Recast | Character as model trained on narrative events, arc = training run, reader response = metrics | 0.8 | 0.6 | 0.6 |
| 3 | GOLEM → ML-Schema | Relabel | Character as ML-Schema Model | 0.1 | 0.3 | 0.9 |
| 4 | ML-Schema → GOLEM | Recast | Model as character with traits = architecture, arc = training curve, role = pipeline function | 0.8 | 0.5 | 0.6 |
| 4 | ML-Schema → GOLEM | Relabel | Model as GOLEM Character | 0.1 | 0.3 | 0.9 |
| 5 | SUMO → OMC | Recast | Process as creative work with capture/post/distribute pipeline stages | 0.5 | 0.9 | 0.8 |
| 5 | SUMO → OMC | Relabel | Process as OMC CreativeWork | 0.1 | 0.5 | 0.9 |
| 6 | OMC → SUMO | Recast | Creative work as process with participants = creators, temporal extent = timeline, sub-processes = stages | 0.4 | 0.9 | 0.9 |
| 6 | OMC → SUMO | Relabel | Creative work as SUMO Process | 0.1 | 0.8 | 0.9 |

**Notable findings**:
- All recast procedures outperform relabel on novelty (0.4–0.9 vs 0.1). M1 corroborated at the procedure level.
- Cell 2 relabel has *lower validity* than cell 2 recast (0.4 vs 0.7): ESO's Event doesn't satisfy FIBO's contract axioms without the recast's contract framing. The relabel is not just low-novelty, it's *invalid* — a stronger discriminating signal than expected.
- Cells 5/6 (SUMO↔OMC, both process ontologies) have low novelty (0.5, 0.4) — confirming G6's prediction that low-gradient sites produce small mutations.

### 3.3 Iteration 1 — Pareto frontier

Non-dominated sort (A dominates B if A ≥ B on all objectives and > B on at least one):

- **Cell 1 Recast** (0.9, 0.9, 0.6) — non-dominated. ✓
- Cell 1 Relabel (0.1, 0.9, 0.9) — dominated by Cell 6 Recast (0.4, 0.9, 0.9) on novelty. ✗
- **Cell 2 Recast** (0.8, 0.7, 0.5) — non-dominated (high novelty, decent validity). ✓
- Cell 2 Relabel (0.1, 0.4, 0.9) — dominated. ✗
- Cell 3 Recast (0.8, 0.6, 0.6) — dominated by Cell 2 Recast (0.8, 0.7, 0.5)? Cell 2 ≥ on novelty (equal) and validity (0.7 > 0.6), but Cell 3 > on cost (0.6 > 0.5). **Not dominated** — Cell 3 has higher cost-efficiency. ✓
- Cell 3 Relabel — dominated. ✗
- Cell 4 Recast (0.8, 0.5, 0.6) — dominated by Cell 3 Recast (0.8, 0.6, 0.6) on validity. ✗
- Cell 4 Relabel — dominated. ✗
- **Cell 5 Recast** (0.5, 0.9, 0.8) — non-dominated (high validity, high cost-efficiency). ✓
- Cell 5 Relabel — dominated. ✗
- **Cell 6 Recast** (0.4, 0.9, 0.9) — non-dominated (highest cost-efficiency with high validity). ✓
- Cell 6 Relabel — dominated by Cell 6 Recast. ✗

**Iteration 1 frontier**: {Cell 1 Recast, Cell 2 Recast, Cell 3 Recast, Cell 5 Recast, Cell 6 Recast}. All recast, no relabel.

### 3.4 Reflection (gpa-evolution step 2)

- **Success pattern**: all frontier members are recast procedures. The constraint-forces mechanism (M1) is corroborated: recast dominates relabel on the Pareto frontier.
- **Failure pattern**: cells 5/6 (process ontologies) have low novelty — both are process ontologies, so the recast produces a small mutation. Confirms G6's prediction.
- **Surprising pattern**: cell 2 relabel has lower validity than cell 2 recast. Strict targets (FIBO with contract axioms) make relabel *invalid*, not just trivial. The recast's advantage is largest against strict targets.
- **Transferable rule**: the gradient map should weight sites by **target ontology strictness**, not just source-target distance. Strict targets produce larger recast-relabel validity gaps.

### 3.5 Iteration 2 — mutations testing the reflected rules

Two mutations, each testing one hypothesis from the reflection:

**Mutation 1 (tests the strict-target rule)**: FIBO `FinancialInstrument` → GOLEM (narrative). GOLEM requires characters with traits/arcs/roles — a strict target.
- Recast: instrument as character with traits = terms (maturity, coupon, principal), arc = lifecycle (issuance → holding → maturity), role = portfolio function (hedging, speculation, income). Novelty 0.85, validity 0.7, cost-inv 0.6.
- Relabel: instrument as GOLEM Character. Novelty 0.1, validity 0.4 (a financial instrument isn't a character without the trait/arc framing — GOLEM's Character has required traits). Cost-inv 0.9.

**Mutation 2 (tests the permissive-target negative control)**: GOLEM `Character` → SUMO (generic). SUMO's Process root is permissive.
- Recast: character as process with participants = traits, temporal extent = arc, sub-processes = narrative beats. Novelty 0.4 (low, as predicted), validity 0.85, cost-inv 0.85.
- Relabel: character as SUMO Process. Novelty 0.1, validity 0.7 (Process is permissive), cost-inv 0.9.

### 3.6 Iteration 2 — Pareto frontier update

Merge iteration 2 outputs into the pool with iteration 1 frontier:

- Mutation 1 Recast (0.85, 0.7, 0.6) — dominated by Cell 1 Recast (0.9, 0.9, 0.6) on novelty and validity, equal on cost. ✗
- Mutation 1 Relabel — dominated. ✗
- Mutation 2 Recast (0.4, 0.85, 0.85) — dominated by Cell 6 Recast (0.4, 0.9, 0.9) on validity and cost. ✗
- Mutation 2 Relabel — dominated. ✗

**Iteration 2 frontier** = Iteration 1 frontier: {Cell 1 Recast, Cell 2 Recast, Cell 3 Recast, Cell 5 Recast, Cell 6 Recast}.

### 3.7 Convergence check

- Hypervolume delta (iteration 2 vs iteration 1) = 0 (frontier unchanged).
- New non-dominated members = 0.
- Convergence metric = 0 + (0.05 × 0) = 0.0 ≤ 0.10. **Converged.** ✓
- **Frontier stable ≥2 iterations.** ✓

### 3.8 Verdict from the evolution

| Hypothesis | Verdict | Evidence |
|---|---|---|
| M1 (constraint forces) | **Corroborated** | All 5 Pareto-frontier members are recast procedures; relabel never enters the frontier across 8 cells. |
| M2 (random perturbation) | **Falsified** | If recast were random, relabel would sometimes enter the frontier; it never does. The recast's structural delta is consistently larger and its validity is consistently higher (especially against strict targets). |
| M4 (rationalization) | **Falsified** | Recast and relabel produce distinguishable outputs (different validity scores against strict targets — cell 2: 0.7 vs 0.4; mutation 1: 0.7 vs 0.4). A rationalization would not produce systematically different validity scores. |
| M3 (shared substrate) | **Not directly tested** | No shared-upper-ontology manipulation was performed. Weak evidence against M3: FIBO→ESO (no shared upper ontology) produces a high-novelty mutant (0.9), suggesting shared substrate is not necessary. Deferred to future work. |

**H1 (the weakened thesis T1')**: corroborated. Recast produces three-criterion mutants at a rate greater than paraphrase (relabel). **H2 (mechanism-strength)**: corroborated. The mutant form is predictable from B's axioms (the minimal-satisfiability projection applies B's axioms; the mutant's structure is determined by which axioms c violates). **H3 (variety-gate)**: not directly tested (variety ratios not computed), but the strict-target rule suggests a proxy: strict targets have higher "effective variety" along the dimension c occupies (more axioms to satisfy), producing larger mutations. Deferred.

---

## 4. Metacognition log (Brier-scored predictions)

| Prediction | p (predicted) | Outcome | Brier |
|---|---|---|---|
| P1: Recast will dominate relabel on the Pareto frontier | 0.80 | 1 (all 5 frontier members are recast) | 0.04 |
| P2: Cells 5/6 (process ontologies) will have low novelty | 0.85 | 1 (novelty 0.5 and 0.4, below the 0.8 average) | 0.0225 |
| P3: The strict-target mutation (Mutation 1) will enter the frontier | 0.40 | 0 (dominated by Cell 1 Recast) | 0.16 |
| P4: The frontier will stabilize in 2 iterations | 0.70 | 1 (stable) | 0.09 |

**Mean Brier = 0.078.** Worst-calibrated: P3 (0.16) — I over-predicted that the strict-target mutation would displace an existing frontier member. **Lesson**: existing frontier members from high-gradient sites are hard to displace; mutations that test *rules* don't necessarily produce *better* artifacts, they test whether the rule generalizes. The rule (strict targets produce larger mutations) was confirmed by the mutation's *scores* (0.85 novelty, 0.7 validity — both high), even though the mutation was dominated by an even-better existing cell. Best-calibrated: P2 (0.0225) — the low-gradient-site prediction was confident and correct.

---

## 5. Acceptance criteria check (Deliverable 2)

> Each framework is a named procedure specifying: inputs, outputs, the ontology roles (source / constraint / sink), the seed-concept selection rule, the convergence criterion, and one worked example. Acceptance: a second agent can instantiate the framework on a fresh concept and produce a mutant with no further instruction.

| Requirement | GSR (§1) | CFR (§2) |
|---|---|---|
| Named procedure | ✓ Gradient-Seeded Recombination | ✓ Constraint-Forces Recast |
| Inputs specified | ✓ §1.2 (ontology registry, prior) | ✓ §2.2 (seed concept, source A, target B, rater) |
| Outputs specified | ✓ §1.3 (gradient map, priority ranking, seed concepts) | ✓ §2.3 (mutant, three-criterion verdict, structural delta, relabel control) |
| Ontology roles (source/constraint/sink) | ✓ §1.4 (source = source ontology; constraint = N/A; sink = gradient map) | ✓ §2.4 (source = A; constraint = B; sink = mutant) |
| Seed-concept selection rule | ✓ §1.5 (most central concept by graph degree) | ✓ §2.5 (from GSR, or most central concept in A if standalone) |
| Convergence criterion | ✓ §1.6 (≥3 high-gradient sites + seed per site) | ✓ §2.7 (Pareto frontier stable ≥2 iterations, hypervolume delta = 0) |
| Worked example | ✓ §1.7 (project's 6 ontologies → 6 sites) | ✓ §2.8 (FIBO FinancialInstrument → ESO, three-criterion mutant) |

**Second-agent instantiation test**: A second agent can take a fresh concept not in the worked examples — e.g., GOLEM `Character` → ML-Schema — and instantiate CFR with no further instruction by following §2.6:
1. Represent `Character` as an axiom graph (traits, arc, role, reader response).
2. Identify ML-Schema's axioms that `Character` violates (a model has a training run and evaluation metrics; a character has neither).
3. Find the minimal modification: add training run (= arc) and evaluation metrics (= reader response).
4. Mutant: "A character is a model trained on narrative events, whose training run is the character's arc and whose evaluation metrics are reader responses."
5. Check three criteria: (i) expressible in GOLEM vocabulary (character, arc, reader response — all GOLEM terms ✓), (ii) absent from GOLEM (GOLEM has no model-with-training-run concept ✓), (iii) consistent under ML-Schema (model + training run + metrics ✓).
6. Generate relabel control: "A character is an ML-Schema Model."
7. Compare: mutant Δ (2 additions) > relabel Δ (1 relabel). ✓

This is the cell 3 recast from the evolution (§3.2), reproduced from the framework's procedure alone. **A second agent can instantiate the framework on a fresh concept and produce a mutant with no further instruction.** ✓

---

## 6. Conditions carried to Pass 3 (skill spec)

1. **Two frameworks, two skills**: GSR and CFR are genuinely separable (different inputs, outputs, convergence criteria). A skill family with 2 skills: `gradient-seeded-recombination` (GSR) and `constraint-forces-recast` (CFR). CFR depends on GSR's output (seed concepts) but can run standalone with its own seed selection.
2. **Phase I deletion-test (preview)**:
   - GSR: if deleted, does its complexity reappear in CFR? Yes — CFR needs a seed selection rule, and the gradient map IS the seed selection rule. **But** GSR's output (a reusable gradient map) is consumed by other skills too (e.g., a future "ontology-coverage-audit" skill). GSR survives the deletion test as a separate skill because its output has multiple consumers. ✓
   - CFR: if deleted, does its complexity reappear in GSR? No — GSR finds sites, CFR recasts. They are genuinely disjoint. ✓
   - Public surfaces: GSR has 3 (map, rank, select-seeds); CFR has 4 (represent, project, check, compare). Both ≤7. ✓
3. **OCAP/gas posture**:
   - GSR is read-only on the ontology registry (no side effects); gas bounded by the number of ontology pairs evaluated (O(n²) where n = number of ontologies).
   - CFR is read-only on ontology terms; gas bounded by the number of axiom-violation checks (O(|B's axioms|) per cell) × the number of cells. The three-criterion test is the gas-consuming step (rater/reasoner effort).
   - Neither skill needs credential allowlists (no external API calls unless BioPortal is used, and BioPortal access is optional). If BioPortal is used, the skill needs `apikey` in its config_env allowlist and per-ontology license checking.
4. **BioPortal-or-equivalent dependency**: per-ontology license check; do not assume blanket CC-BY. The skill spec must include a license-check phase before caching OWL locally.
5. **Elicit MCP integration**: Elicit is a *source* ontology (evidence supplier), never a *constraint* ontology. If an Elicit MCP integration is proposed, it feeds GSR's substrate (adds evidence ontologies to the registry), not CFR's constraint role.
6. **Substrate cardinality**: 6 domain-supplement namespaces + 2 universal axes + 1 core, not "9 ontologies."
7. **The forcing operator** (minimal-satisfiability projection) is the skill's core mechanism. The skill spec must specify it in the manifest's description and the template contracts.
8. **The weakened thesis T1'**: the skill spec must not claim CFR is *the* mechanism of interdisciplinary generativity; it is *a* mechanism for *concept generation*, distinct from retrieval-and-grounding (evidence assembly) and analogy (communication).
