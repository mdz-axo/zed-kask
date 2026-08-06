# Adjacent-Possible Generative-Science Skill Family: Pass 3 Spec

**Status**: Pass 3 of 3 (report → frameworks → **skill spec**). This document specifies the skill family, runs the Phase I deletion-test verdict on each proposed skill, and runs Phase J (skill-discovery) to confirm no existing skill already covers the scope.

**Date**: 2026-08-06

**Predecessors**: `interdisciplinary-constraint-forces-report.md` (Pass 1), `interdisciplinary-constraint-forces-frameworks.md` (Pass 2). This document assumes familiarity with the two frameworks (GSR, CFR), the forcing operator (minimal-satisfiability projection), the weakened thesis T1', and the Phase F evolution results.

---

## 0. Skill family overview

Two skills, genuinely separable (Pass 2, §6):

| Skill                                 | Framework       | Phase(s) covered               | Purpose                                                                                                                                                               |
| ------------------------------------- | --------------- | ------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `gradient-seeded-recombination` (GSR) | GSR (Pass 2 §1) | D (Map substrate)              | Find _where_ to apply constraint-forces recast — inventories ontologies, maps recombination gradients, prioritizes sites, selects seed concepts                       |
| `constraint-forces-recast` (CFR)      | CFR (Pass 2 §2) | F (Evolve) + the recast itself | The core generative framework — recasts a seed concept into a target ontology's constraints via minimal-satisfiability projection, producing a three-criterion mutant |

**Dependency**: CFR consumes GSR's output (seed concepts + target assignments) but can run standalone with its own seed selection (Pass 2 §2.5). The two skills are not bundled — a user can invoke GSR to produce a gradient map without recasting, or invoke CFR directly with a manually-specified seed and target.

**Why two skills, not one**: GSR's output (a reusable gradient map) has multiple consumers — CFR is one, but a future "ontology-coverage-audit" or "interdisciplinary-opportunity-tracker" could also consume it. Bundling would force the gradient map to be recomputed every time CFR runs. The Phase I deletion test (§3) confirms the separation.

---

## 1. Skill 1 — `gradient-seeded-recombination` (GSR)

### 1.1 Manifest sketch

```yaml
manifest:
  id: gradient-seeded-recombination
  category: skill
  name: Gradient-Seeded Recombination
  description: >
    Convergent substrate-analysis process for interdisciplinary concept
    generation. Inventories a set of ontology namespaces (the project's
    6 domain-supplement namespaces + 2 universal axes + 5W1H core, or
    extended via BioPortal-or-equivalent OBO/OWL sources with per-ontology
    license check), builds a complete-graph prior (every ontology should
    have a recombination surface with every other), maps the actual
    recombination field, detects gradients between populated and
    unpopulated regions using the gradient-hunter eight-shape taxonomy
    (sharp cliff, roof edge, wombling boundary, regression discontinuity,
    topological hole, oracle gap, frustrated landscape, allosteric
    population shift), generates reason hypotheses via the seven-class
    taxonomy (Rubin MCAR/MAR/MNAR + spin glass metastable trap + allostery
    broken coupling), prioritizes sites by reason class then fractal
    recurrence then magnitude, and selects seed concepts (most central
    concept by graph degree in the source ontology). Produces a reusable
    gradient map consumed by constraint-forces-recast and by future
    ontology-coverage-audit skills. Distinct from gradient-hunter: GSR
    hunts gradients *between ontologies* (cross-ontology recombination
    sites), while gradient-hunter hunts gradients *within a codebase or
    telemetry field* (test coverage, span emission, hook wiring). The
    substrate is ontology namespaces, not code artifacts.
  functional_role: flowdef
  version: 0.33.0
  editor: curator-or-human-admin
  visibility: Public

convergence:
  convergence_mode: "cauchy"
  cauchy_epsilon: 0.03
  cauchy_window: 3
  max_iterations: 3
  min_iterations: 2
  on_not_reached: escalate

gas:
  cap: 80000
  cost_per_iteration: 100
  alert_threshold: 0.8
  hard_limit: true

rjoule:
  cap: 2
  alert_threshold: 0.8
  hard_limit: true

inputs:
  - name: ontology_registry
    type: object
    required: true
    description: >
      The set of ontology namespaces to inventory. Each entry carries
      the namespace id (fibo, eso, golem, mlschema, omc, sumo, pko,
      dc_bibo, or a BioPortal-or-equivalent acronym), its axioms, its
      term signature, and its key concepts. The project's default
      registry is the 6 domain-supplement namespaces + 2 universal axes;
      BioPortal extensions require per-ontology license check before
      caching OWL locally.
  - name: ontology_sources
    type: array
    required: false
    description: >
      External ontology sources to extend the registry beyond the
      project's 6 domain-supplement namespaces. Each entry names a
      provider ("obo_foundry" | "ontobee" | "bioportal") and optional
      auth. OBO Foundry is the default and requires no auth (direct OWL
      at http://purl.obolibrary.org/obo/{acronym}.owl, registry YAML
      at https://obofoundry.org/registry/ontologies.yml, ~200+
      ontologies, per-ontology license visible in the registry).
      OntoBee mirrors OBO Foundry with a SPARQL endpoint.
      BioPortal aggregates 1,288 ontologies but requires an apikey
      (free NCBO registration, passed as config_env not credentials).
      Per-ontology license checking is a Guardrail enforced in the
      Inventory phase regardless of provider — OBO Foundry licenses
      span CC-BY 4.0, CC-BY 3.0, CC0, Apache 2.0, GPL-3.0, Artistic-2.0;
      do not assume blanket CC-BY.
  - name: bioportal_apikey
    type: string
    required: false
    description: >
      Optional BioPortal API key (free NCBO registration). Only used
      when ontology_sources includes "bioportal". config_env entry, not
      a credential.

ledger:
  span_namespace: reg.skill.gradient-seeded-recombination
```

### 1.2 Phase(s) covered

GSR covers **Phase D (Map substrate)** of the mission's decomposition. It is the substrate-analysis phase that produces the gradient map and seed concepts consumed by Phase F (Evolve, run by CFR).

### 1.3 PDCA shape (emergent from ontological anchors, per create-skill)

The PDCA shape emerges from the gradient-hunter substrate ontology (Parisi spin glass theory) + the cross-ontology recombination domain:

```
Plan:   Phase 1 — Inventory    → Enumerate ontology namespaces + their key concepts
Plan:   Phase 2 — Prior        → Build complete-graph prior K_n (every pair should recombine)
Do:     Phase 3 — Map          → Map actual recombination field (which pairs have populated surfaces)
Do:     Phase 4 — Detect       → Classify gradients by the 8-shape taxonomy + fractal recurrence
Check:  Phase 5 — Hypothesize  → Generate reason hypotheses (7-class taxonomy) per gradient
Check:  Phase 6 — Prioritize   → Rank sites by reason class > fractal recurrence > magnitude
Act:    Phase 7 — Select seeds → Pick most-central concept per high-priority site
Check:  Phase 8 — Converge     → Cauchy criterion on gradient map stability
Act:    Phase 9 — Loop         → If not converged, re-enter at Phase 2 with refined prior
```

This shape is _not_ copied from gradient-hunter's Prior→Map→Detect→Hypothesize→Report→Convergence. It emerges from the cross-ontology substrate: the _Inventory_ phase (enumerate ontologies) has no analog in gradient-hunter (which inventories code artifacts, not ontologies); the _Select seeds_ phase (pick most-central concept) has no analog in gradient-hunter (which reports gradients, not seeds for downstream recast). The shape is idiosyncratic to GSR's domain, per the create-skill anti-pattern rule ("do not copy bug-hunt pattern for non-bug-hunt skill").

### 1.4 OCAP / gas posture

- **OCAP**: GSR is read-only on the ontology registry. It does not mutate ontologies, create concepts, or invoke external APIs with side effects. The only external call is BioPortal download (read-only GET). No credential allowlist needed beyond `bioportal_apikey` (config_env, not credentials — it's a free registration key, not a secret). If BioPortal is used, the skill must check `hasLicense` per acronym before caching OWL locally; this is a Guardrail (per-ontology license compliance), enforced in the Inventory phase.
- **Gas**: bounded by O(n²) ontology-pair evaluations where n = number of ontologies. For the project's 6 namespaces, n² = 36 pairs (manageable). For BioPortal-extended registries (n up to ~20), n² = 400 pairs — the gas cap (80,000) bounds this. The alert threshold (0.8) fires at 64,000 gas, prompting the operator to narrow the registry.
- **rjoule**: cap 2 (low — GSR is substrate analysis, not generation; the heavy rjoule consumer is CFR).

### 1.5 Template contracts (ontology-anchored types)

Per create-skill's ontological anchoring pattern, the template contracts use the gradient-hunter ontology's entity types (gradient shapes, reason classes) extended with the cross-ontology recombination types:

| Template              | Type    | Contract fields (ontology-anchored)                                                                                                                           |
| --------------------- | ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `gsr-inventory.j2`    | KnowAct | `ontology_registry: OntologyNamespace[]`, `key_concepts: Concept[]` (PKO: `pko:Procedure` = the inventory procedure; DC: `dcterms:identifier` = namespace id) |
| `gsr-prior.j2`        | KnowAct | `prior: CompleteGraph` (gradient-hunter: prior model; pragmatic-cybernetics: variety engineering when no convention prior)                                    |
| `gsr-map.j2`          | KnowAct | `actual_field: RecombinationSurface[]` (gradient-hunter: field map; graph-audit: topology extraction)                                                         |
| `gsr-detect.j2`       | KnowAct | `gradients: Gradient[]` with `shape: GradientShape` (8-shape taxonomy), `fractal_recurrence: bool`                                                            |
| `gsr-hypothesize.j2`  | KnowAct | `gradient_hypotheses: ReasonHypothesis[]` (7-class taxonomy; delegates to falsifiability for counterfactual discrimination)                                   |
| `gsr-prioritize.j2`   | KnowAct | `priority_ranking: RankedSite[]` (reason class > fractal recurrence > magnitude > populated-side criticality)                                                 |
| `gsr-select-seeds.j2` | KnowAct | `seed_concepts: SeedConcept[]` with `concept: Concept`, `source_ontology: OntologyNamespace`, `centrality: float` (graph degree)                              |
| `gsr-converge.j2`     | compute | Cauchy criterion on gradient map stability (deterministic, no LLM)                                                                                            |

**Public surface count**: 8 templates. **This exceeds the ≤7 rule.** Phase I deletion test (§3) addresses this.

### 1.6 Span namespaces

- Ledger span namespace: `reg.skill.gradient-seeded-recombination` (CI-enforced).
- Per-template `generates_spans` (ontology-derived short names): `reg.gsr.inventory`, `reg.gsr.prior`, `reg.gsr.map`, `reg.gsr.detect`, `reg.gsr.hypothesize`, `reg.gsr.prioritize`, `reg.gsr.select_seeds`, `reg.gsr.converge`.

---

## 2. Skill 2 — `constraint-forces-recast` (CFR)

### 2.1 Manifest sketch

```yaml
manifest:
  id: constraint-forces-recast
  category: skill
  name: Constraint-Forces Recast
  description: >
    Convergent concept-generation process for interdisciplinary research.
    Recasts a seed concept from a source ontology A into a target
    ontology B's constraint context via minimal-satisfiability projection
    (the mutant is the nearest model of B to the seed concept, measured
    by structural delta / graph-edit distance). Produces a three-criterion
    mutant that is (i) expressible in A's signature (novel compounds
    permitted iff compositional — meaning is a function of parts per
    A's existing composition rules, no new axioms), (ii) absent from A
    (not subsumed by any existing A concept), (iii) consistent under B's
    axioms (satisfies all of B's axioms, verified by OWL reasoner or
    human rater). Generates a relabel control (vocabulary swap without
    axiom application) and compares structural deltas — a valid mutant
    must outperform the relabel on structural delta, discriminating
    constraint-forces (M1) from random perturbation (M2). Runs an
    evolutionary loop over a seed set (from gradient-seeded-recombination
    or manually specified), keeping the Pareto frontier on (novelty,
    validity, cost-inverted) stable for ≥2 iterations. Anchored to
    Anchored to Popper falsifiability (the three-criterion test is the falsifier),
    Platt strong inference (the relabel control is the discriminating
    test), and Pearl counterfactuals (the relabel is the do(not recast)
    counterfactual). The forcing operator is minimal-satisfiability
    projection — not entailment (too strong, projection not generation)
    and not bare satisfiability (too weak, doesn't discriminate M1 from
    M2). The weakened thesis T1': constraint-forces recasting is *a*
    mechanism for interdisciplinary *concept generation*, distinct from
    retrieval-and-grounding (evidence assembly, e.g. Elicit) and analogy
    (communication). Does not claim to be the only interdisciplinary
    operation. Substrate ontologies are sourced via a multi-provider
    abstraction (OBO Foundry primary, OntoBee mirror, BioPortal
    aggregator) — see gradient-seeded-recombination's ontology_sources
    input.
  functional_role: flowdef
  version: 0.33.0
  editor: curator-or-human-admin
  visibility: Public

convergence:
  convergence_mode: "cauchy"
  cauchy_epsilon: 0.03
  cauchy_window: 3
  max_iterations: 5
  min_iterations: 2
  on_not_reached: escalate

gas:
  cap: 150000
  cost_per_iteration: 100
  alert_threshold: 0.8
  hard_limit: true

rjoule:
  cap: 4
  alert_threshold: 0.8
  hard_limit: true

inputs:
  - name: seed_concepts
    type: object
    required: true
    description: >
      Seed concepts to recast. Each entry carries the concept (with its
      axiom graph), the source ontology A (with its signature), and the
      target ontology B (with its axioms). When gradient-seeded-
      recombination has been run, this is its seed_concepts output. When
      CFR runs standalone, the seed is the most-central concept in A
      (highest graph degree) and B is an ontology with high gradient
      from A (per GSR) or, if no gradient map, an ontology with strict
      axioms (strict targets produce larger mutations; permissive
      targets produce trivial mutants — confirmed by the Phase F
      evolution in the frameworks pass).
  - name: rater
    type: string
    required: false
    description: >
      The rater mode: "human" (default) or "reasoner". In "human" mode,
      the three-criterion test is judged by a human rater approximating
      the argmin of the minimal-satisfiability projection. In "reasoner"
      mode, an OWL reasoner (Hermit, Pellet) checks satisfiability
      mechanically; the minimality still requires a distance metric on
      concept structures (graph-edit distance), which the reasoner does
      not provide — reasoner mode is therefore partial and falls back
      to human judgment for the minimality step.

ledger:
  span_namespace: reg.skill.constraint-forces-recast
```

### 2.2 Phase(s) covered

CFR covers **Phase F (Evolve)** of the mission's decomposition, plus the recast operation itself (which is the core generative act, not a named phase in the mission's A–J decomposition — it sits between D and F as the mutation operator that F evolves over).

### 2.3 PDCA shape (emergent from ontological anchors)

The PDCA shape emerges from the falsifiability substrate (Popper/Platt/Pearl) + the minimal-satisfiability-projection forcing operator:

```
Plan:   Phase 1 — Represent    → Represent seed concept c as an axiom graph
Plan:   Phase 2 — Violate      → Identify B's axioms that c violates
Do:     Phase 3 — Project      → Find minimal-satisfiability projection (the mutant m = argmin Δ(c, m))
Do:     Phase 4 — Control      → Generate relabel control (vocabulary swap, no axiom application)
Check:  Phase 5 — Three-criterion → Check (i) expressible in A's signature, (ii) absent from A, (iii) consistent under B
Check:  Phase 6 — Compare      → Mutant Δ > relabel Δ (else M1 falsified for this cell)
Act:    Phase 7 — Frontier     → Update Pareto frontier on (novelty, validity, cost-inverted)
Check:  Phase 8 — Converge     → Cauchy criterion on frontier stability (≥2 iterations)
Act:    Phase 9 — Loop         → If not converged, re-enter at Phase 1 with next seed from frontier
```

This shape is _not_ copied from falsifiability's admit→hypothesize→counterfactual→discriminate→eliminate. It emerges from the recast substrate: the _Represent_ phase (axiom graph) has no analog in falsifiability (which represents claims, not concept graphs); the _Project_ phase (minimal-satisfiability) is the forcing operator, which has no analog in falsifiability (which eliminates, not generates). The shape is idiosyncratic to CFR's domain.

### 2.4 OCAP / gas posture

- **OCAP**: CFR is read-only on ontology terms. It does not mutate ontologies or invoke external APIs. The recast is a pure function: `(c, A, B) → mutant`. No credential allowlist needed. If BioPortal is used to fetch B's axioms, the same per-ontology license check as GSR applies (Guardrail).
- **Gas**: bounded by O(|B's axioms| × number of cells) for the violation check, plus O(frontier size × iterations) for the evolutionary loop. For the Phase F evolution (6 cells × 2 procedures × 2 iterations = 16 outputs), gas was well under the 150,000 cap. The alert threshold (0.8) fires at 120,000, prompting the operator to narrow the seed set.
- **rjoule**: cap 4 (higher than GSR — CFR is the generative step; the three-criterion test and the evolutionary loop are rjoule-intensive).

### 2.5 Template contracts (ontology-anchored types)

| Template                 | Type    | Contract fields (ontology-anchored)                                                                                                                              |
| ------------------------ | ------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cfr-represent.j2`       | KnowAct | `concept: Concept`, `axiom_graph: Graph` (PKO: `pko:Step` = represent; DC: `dcterms:identifier` = concept id)                                                    |
| `cfr-violate.j2`         | KnowAct | `violations: AxiomViolation[]` (falsifiability: the violations are the falsifiable predictions; ESO: `eso:Event` = the violation event with pre/post situations) |
| `cfr-project.j2`         | KnowAct | `mutant: Concept`, `structural_delta: int` (the forcing operator: `argmin_{m ∈ Models(B)} Δ(c, m)`)                                                              |
| `cfr-control.j2`         | KnowAct | `relabel: Concept`, `relabel_delta: int` (falsifiability: the do(not recast) counterfactual)                                                                     |
| `cfr-three-criterion.j2` | KnowAct | `verdict: {expressible_in_A, absent_from_A, consistent_under_B}` (falsifiability: the three-criterion test is the falsifier)                                     |
| `cfr-compare.j2`         | KnowAct | `comparison: {mutant_delta, relabel_delta, m1_corroborated: bool}` (Platt: discriminating test)                                                                  |
| `cfr-frontier.j2`        | KnowAct | `frontier: ParetoFrontier` with `novelty, validity, cost_inverted` (gpa-evolution: non-dominated sort)                                                           |
| `cfr-converge.j2`        | compute | Cauchy criterion on frontier stability (deterministic, no LLM)                                                                                                   |

**Public surface count**: 8 templates. **This exceeds the ≤7 rule.** Phase I deletion test (§3) addresses this.

### 2.6 Span namespaces

- Ledger span namespace: `reg.skill.constraint-forces-recast` (CI-enforced).
- Per-template `generates_spans`: `reg.cfr.represent`, `reg.cfr.violate`, `reg.cfr.project`, `reg.cfr.control`, `reg.cfr.three_criterion`, `reg.cfr.compare`, `reg.cfr.frontier`, `reg.cfr.converge`.

---

## 3. Phase I — Essentialist deletion-test verdicts

Running the 3-gate protocol (G1 Exist → G2 Surface → G3 Contract) on each proposed skill.

### 3.1 GSR — Gate 1 (Exist / Deletion Test)

**From the caller perspective** (inline GSR's logic into CFR): if GSR is deleted, CFR needs a seed selection rule. CFR's standalone seed selection (Pass 2 §2.5: "most central concept in A, target = high-gradient ontology") is a _degenerate_ version of GSR's gradient map — it picks one seed without mapping the full recombination field. Complexity reappears in CFR: CFR would need to re-implement the gradient map to do seed selection well. **Behavior IS lost on deletion** (the reusable gradient map is lost; CFR gets a degenerate seed selector).

**From the artifact perspective** (delete GSR, replace with direct calls to gradient-hunter): gradient-hunter hunts gradients in a codebase/telemetry field, not between ontologies. GSR's substrate (ontology namespaces) is different from gradient-hunter's substrate (code artifacts). Direct calls to gradient-hunter would not produce a cross-ontology gradient map — gradient-hunter has no notion of "ontology namespace" as a field element. **Behavior IS lost on deletion** (the cross-ontology gradient map is lost; gradient-hunter produces a code-gradient map, not an ontology-gradient map).

**G1 verdict: PASS.** GSR survives the deletion test. Its output (the reusable cross-ontology gradient map) is not trivially reproducible by CFR or by gradient-hunter.

### 3.2 GSR — Gate 2 (Surface / Interface Count)

Public surfaces: 8 templates (§1.5). **Exceeds the ≤7 rule by 1.**

**Reduction attempt**: can `gsr-prioritize.j2` be merged into `gsr-detect.j2`? Detect classifies gradients by shape; prioritize ranks them by reason class. These are different operations (classification vs ranking) with different outputs (gradient list vs ranked list). Merging would conflate two distinct steps and produce a template that does both poorly.

**Reduction attempt**: can `gsr-converge.j2` be merged into `gsr-select-seeds.j2`? Convergence is a deterministic compute step (Cauchy criterion); seed selection is a KnowAct (picks the most-central concept). These are different template _types_ (compute vs KnowAct). Merging would violate the type discipline.

**Justification for the 8th surface**: `gsr-prioritize.j2` is kept separate because prioritization is consumed by multiple downstream consumers (CFR's seed selection, a future ontology-coverage-audit's gap report, a future interdisciplinary-opportunity-tracker's dashboard). A merged detect+prioritize template would force all consumers to take both outputs when they want only one. **Written justification provided per the essentialist rule.**

**G2 verdict: PASS with justification.** 8 surfaces, 1 beyond the limit, justified by multi-consumer output separation.

### 3.3 GSR — Gate 3 (Contract / Abstraction Trace)

- **Traits**: GSR has no traits (it's a FlowDef skill, not a port). N/A.
- **Wrappers/adapters**: GSR's delegation to gradient-hunter (for the 8-shape taxonomy) and to falsifiability (for counterfactual discrimination) is genuine delegation, not pass-through — GSR adds the cross-ontology substrate that gradient-hunter lacks. Not a pass-through.
- **Config structs**: `ontology_registry` and `bioportal_apikey` are inputs, not pass-through config. The `bioportal_apikey` is used in the Inventory phase to fetch OWL; it's not passed through untouched.
- **Error types**: GSR's errors (license-check failure, ontology-not-found) are domain-specific, not wrappers around a single inner error.
- **Generic parameters**: GSR has no generics. N/A.

**G3 verdict: PASS.** No pass-through abstractions.

### 3.4 GSR — Overall essentialist verdict

**G1 PASS, G2 PASS with justification, G3 PASS.** GSR is essential. Essentialism score: 0% (no items removed — already minimal modulo the justified 8th surface).

### 3.5 CFR — Gate 1 (Exist / Deletion Test)

**From the caller perspective** (inline CFR's logic into GSR): if CFR is deleted, GSR produces a gradient map but cannot recast. The recast operation (minimal-satisfiability projection) is the core generative act; GSR is substrate analysis. Inlining CFR into GSR would force GSR to do both substrate analysis and concept generation — a deep-module violation (GSR's interface would balloon from "produce a gradient map" to "produce a gradient map and recast concepts"). **Behavior IS lost on deletion** (the recast operation is lost; GSR becomes substrate-only).

**From the artifact perspective** (delete CFR, replace with direct calls to falsifiability): falsifiability eliminates hypotheses; it does not generate concepts. The minimal-satisfiability projection (the forcing operator) has no analog in falsifiability. Direct calls to falsifiability would produce eliminations, not mutants. **Behavior IS lost on deletion** (the generative recast is lost; falsifiability produces a falsification log, not a mutant).

**G1 verdict: PASS.** CFR survives the deletion test. Its core operation (minimal-satisfiability projection) is not reproducible by GSR or by falsifiability.

### 3.6 CFR — Gate 2 (Surface / Interface Count)

Public surfaces: 8 templates (§2.5). **Exceeds the ≤7 rule by 1.**

**Reduction attempt**: can `cfr-compare.j2` be merged into `cfr-three-criterion.j2`? The three-criterion test checks (i)/(ii)/(iii); the compare step checks mutant Δ > relabel Δ. These are different checks (criteria vs delta comparison) with different outputs (verdict vs comparison). Merging would conflate the falsifier (three-criterion) with the discriminating test (delta comparison).

**Reduction attempt**: can `cfr-control.j2` be merged into `cfr-project.j2`? Project generates the mutant; control generates the relabel. These are symmetric operations (recast vs relabel) but with different procedures (axiom application vs vocabulary swap). Merging would produce a template that does both, with a flag to switch — a deep-module violation (the interface would need to expose both procedures).

**Justification for the 8th surface**: `cfr-frontier.j2` is kept separate because the Pareto frontier update is a distinct operation (non-dominated sort + crowding-distance pruning) consumed by the convergence check. It follows gpa-evolution's template separation (gpa-evolution has a separate `gpa-frontier-update.j2`). **Written justification provided.**

**G2 verdict: PASS with justification.** 8 surfaces, 1 beyond the limit, justified by operation separation and gpa-evolution precedent.

### 3.7 CFR — Gate 3 (Contract / Abstraction Trace)

- **Traits**: CFR has no traits. N/A.
- **Wrappers/adapters**: CFR's delegation to falsifiability (for the three-criterion test as falsifier) and to gpa-evolution (for the frontier update) is genuine delegation — CFR adds the minimal-satisfiability projection that neither has. Not a pass-through.
- **Config structs**: `seed_concepts` and `rater` are inputs, not pass-through config. The `rater` mode ("human" vs "reasoner") is used in the three-criterion phase; it's not passed through untouched.
- **Error types**: CFR's errors (no-violations-found → degenerate recast, mutant-not-expressible-in-A, frontier-not-stable) are domain-specific.
- **Generic parameters**: CFR has no generics. N/A.

**G3 verdict: PASS.** No pass-through abstractions.

### 3.8 CFR — Overall essentialist verdict

**G1 PASS, G2 PASS with justification, G3 PASS.** CFR is essential. Essentialism score: 0% (no items removed — already minimal modulo the justified 8th surface).

---

## 4. Phase J — Skill-discovery gap analysis

Running skill-discovery's `detect-gap` and `search` to confirm no existing skill already covers GSR's or CFR's scope.

### 4.1 Gap detection

**Task pattern**: "recast a concept from ontology A into ontology B's constraint context to generate a mutant concept that is expressible in A, absent from A, and consistent under B."

**Coverage check against the 95-manifest catalog** (verified via `list_directory` + `grep` for "recast", "recombination", "interdisciplinary", "cross-ontology", "concept mutation", "concept transfer", "adjacent possible", "ontology mapping", "cross-domain", "domain transfer"):

| Existing skill        | Coverage                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | Reason |
| --------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| gradient-hunter       | **Partial — Feature gap, not coverage gap.** gradient-hunter hunts gradients in a codebase/telemetry field (test coverage, span emission, hook wiring). GSR hunts gradients _between ontologies_ (cross-ontology recombination sites). Same gradient-shape taxonomy, different substrate. GSR is not a re-skin of gradient-hunter — the substrate (ontology namespaces) requires an Inventory phase (enumerate ontologies) and a Select-seeds phase (pick most-central concept) that gradient-hunter lacks. |
| falsifiability        | **Partial — Feature gap.** falsifiability eliminates hypotheses; CFR generates concepts. CFR uses falsifiability's three-criterion test as the falsifier, but the forcing operator (minimal-satisfiability projection) is not in falsifiability. CFR is a consumer of falsifiability, not a duplicate.                                                                                                                                                                                                      |
| gpa-evolution         | **Partial — Feature gap.** gpa-evolution evolves text artifacts (prompts); CFR evolves concept-recast artifacts. CFR uses gpa-evolution's Pareto frontier update, but the mutation operator (minimal-satisfiability projection) is not in gpa-evolution. CFR is a consumer of gpa-evolution, not a duplicate.                                                                                                                                                                                               |
| structured-extraction | **No coverage.** Extracts entities/relations from text; does not recast concepts across ontologies.                                                                                                                                                                                                                                                                                                                                                                                                         |
| graph-audit           | **No coverage.** Analyzes dependency graphs; does not recast concepts.                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| wardley-mapper        | **No coverage.** Maps components on evolution axis; does not recast concepts across ontologies.                                                                                                                                                                                                                                                                                                                                                                                                             |
| scenario-builder      | **No coverage.** Builds divergent scenarios; does not recast concepts.                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| mcda                  | **No coverage.** Multi-criteria decision analysis; does not generate concepts.                                                                                                                                                                                                                                                                                                                                                                                                                              |
| sequential-inquiry    | **No coverage.** Chain-of-thought reasoning; does not recast concepts.                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| create-skill          | **No coverage.** Creates skills; does not recast concepts. (create-skill's ontology reference set is consumed by GSR as the substrate, but create-skill does not do recombination.)                                                                                                                                                                                                                                                                                                                         |
| superforecasting      | **No coverage.** The only "ontology mapping" hit in the catalog is superforecasting's `ontology mapping` field, which is about prediction-market calibration, not cross-ontology concept recasting.                                                                                                                                                                                                                                                                                                         |

**Gap classification**: Coverage gap (no existing skill covers cross-ontology concept recasting). GSR and CFR are not feature gaps of gradient-hunter/falsifiability/gpa-evolution — they are new skills that _consume_ those skills as delegates.

**Impact**: high. Cross-ontology concept generation is a capability the mission identifies as generative (the Phase F evolution corroborated M1: recast dominates relabel on the Pareto frontier). No existing skill provides it.

**Recommended action**: `create_skill` for both GSR and CFR.

### 4.2 Gap search (skill-discovery-search)

Scoring the top candidates against GSR's and CFR's capability:

| Candidate                      | Capability match (0.50)                               | Lexicon overlap (0.25)                           | Trigger relevance (0.25)                                              | Fit score | Gap fill type                                                                                                                                                                                                                                                                                        |
| ------------------------------ | ----------------------------------------------------- | ------------------------------------------------ | --------------------------------------------------------------------- | --------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| gradient-hunter (vs GSR)       | 0.55 (same gradient taxonomy, different substrate)    | 0.60 (gradient, prior, map, detect, hypothesize) | 0.40 (gradient-hunter triggers on codebase/telemetry, not ontologies) | 0.54      | **extension** — GSR could extend gradient-hunter with a cross-ontology substrate, but the Inventory and Select-seeds phases are not extensions of gradient-hunter's phases; they are new phases. Extension would bloat gradient-hunter's scope.                                                      |
| falsifiability (vs CFR)        | 0.45 (CFR uses falsifiability's three-criterion test) | 0.35 (falsifier, counterfactual, discriminate)   | 0.30 (falsifiability triggers on claims, not concepts)                | 0.39      | **extension** — CFR could extend falsifiability with a generation operator, but falsifiability is an eliminative engine; adding generation would violate its core constraint ("corroborated is not confirmed; we eliminate, we do not generate"). Extension would corrupt falsifiability's identity. |
| gpa-evolution (vs CFR)         | 0.50 (CFR uses gpa-evolution's frontier update)       | 0.40 (Pareto, frontier, novelty, validity, cost) | 0.35 (gpa-evolution triggers on prompt artifacts, not concepts)       | 0.43      | **extension** — CFR could extend gpa-evolution with a concept-recast artifact type, but gpa-evolution v1 only implements `artifact_type: "prompt"`; adding `concept` would be a v2 feature, not an extension.                                                                                        |
| structured-extraction (vs CFR) | 0.20 (both produce structured outputs)                | 0.15                                             | 0.10                                                                  | 0.16      | below 0.20 threshold — not a candidate                                                                                                                                                                                                                                                               |

**Search coverage**: `weak` (best fit 0.54, below 0.60). No existing skill is a `direct` fill. All three `extension` candidates would corrupt the candidate skill's identity (gradient-hunter's substrate, falsifiability's eliminative core, gpa-evolution's prompt-only v1).

**Verdict**: `create_skill` for both GSR and CFR. The extensions are not viable — they would violate the essentialist G1 deletion test for the candidate skills (extending gradient-hunter with cross-ontology substrate would make gradient-hunter no longer about codebase/telemetry gradients; extending falsifiability with generation would violate its eliminative core).

### 4.3 Phase J convergence

**Convergence signal**: gap list, not dupes. ✓ Achieved — the gap analysis identifies a Coverage gap (no existing skill covers cross-ontology concept recasting), the search confirms no `direct` fill exists, and the `extension` candidates are rejected because they would corrupt the candidate skills' identities. GSR and CFR are new skills, not duplicates.

---

## 5. Acceptance criteria check (Deliverable 3)

> For each proposed skill: manifest sketch, the phase(s) it covers, OCAP/gas posture, and the Phase I deletion-test verdict. Acceptance: Phase J confirms no existing skill already covers it.

| Requirement                                  | GSR (§1)                                                                                                       | CFR (§2)                                                                                                |
| -------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Manifest sketch                              | ✓ §1.1 (id, name, description, functional_role, version, visibility, convergence, gas, rjoule, inputs, ledger) | ✓ §2.1 (same fields)                                                                                    |
| Phase(s) covered                             | ✓ §1.2 (Phase D — Map substrate)                                                                               | ✓ §2.2 (Phase F — Evolve + the recast operation)                                                        |
| OCAP/gas posture                             | ✓ §1.4 (read-only on ontology registry; gas O(n²); rjoule cap 2; BioPortal per-ontology license check)         | ✓ §2.4 (read-only on ontology terms; gas O(\|B's axioms\| × cells); rjoule cap 4; pure-function recast) |
| Phase I deletion-test verdict                | ✓ §3.4 (G1 PASS, G2 PASS with justification, G3 PASS; essentialism 0%)                                         | ✓ §3.8 (G1 PASS, G2 PASS with justification, G3 PASS; essentialism 0%)                                  |
| Phase J confirms no existing skill covers it | ✓ §4 (Coverage gap; search coverage weak; best fit 0.54 < 0.60; extensions rejected)                           | ✓ §4 (same analysis covers both)                                                                        |

**Acceptance criterion met**: Phase J confirms no existing skill already covers GSR's or CFR's scope. ✓

---

## 6. Conditions carried forward (for the operator / future work)

1. **Ontology-source integration is multi-provider and gated**: both skills can run on the project's 6 domain-supplement namespaces with no external dependency. When external ontologies are needed, the substrate is a **provider abstraction** over three verified sources, not a single hard-coded dependency:

   | Provider                    | URL                                 | Auth                              | Download                                                                                                  | License model                                                                                              | Verified status                                                                                                |
   | --------------------------- | ----------------------------------- | --------------------------------- | --------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
   | **OBO Foundry** (primary)   | `https://obofoundry.org`            | None                              | Direct OWL at `http://purl.obolibrary.org/obo/{acronym}.owl`; registry YAML at `/registry/ontologies.yml` | **Per-ontology** — CC-BY 4.0, CC-BY 3.0, CC0, Apache 2.0, GPL-3.0, Artistic-2.0 all appear in the registry | Verified 2026-08-06 — ~200+ ontologies, no auth, direct OWL download, per-ontology license visible in registry |
   | **OntoBee** (mirror/SPARQL) | `https://ontobee.org`               | None evident                      | Linked-data dereference; SPARQL endpoint                                                                  | Inherits OBO Foundry licenses (it's the default dereference server for most OBO library ontologies)        | Verified 2026-08-06 — ~276 ontologies, no auth evident, API terms not deeply verified                          |
   | **BioPortal** (aggregator)  | `https://bioportal.bioontology.org` | `apikey` (free NCBO registration) | `/ontologies/{acronym}/download?download_format=rdf`                                                      | **Per-ontology** via `hasLicense`/`useGuidelines`/`morePermissions` per submission                         | Verified 2026-08-06 (Pass 1) — 1,288 ontologies, requires apikey, per-ontology license                         |
   | **EBI OLS4** (lookup)       | `https://www.ebi.ac.uk/ols4/`       | Unknown                           | Unknown (SPA, requires JS to inspect)                                                                     | Unknown                                                                                                    | Accessible but API terms unverified this session                                                               |
   | **AberOWL**                 | `https://aber-owl.net`              | —                                 | —                                                                                                         | —                                                                                                          | **Unreachable** (connection error 2026-08-06) — do not depend on it                                            |

   **Architecture**: GSR's Inventory phase accepts an `ontology_sources` config listing one or more providers. OBO Foundry is the default (no auth, direct OWL, per-ontology license in the registry YAML). BioPortal is an optional aggregator for non-OBO ontologies (requires `bioportal_apikey` in config_env, not credentials — it's a free registration key). OntoBee is a SPARQL-capable mirror for OBO ontologies. EBI OLS4 is a candidate but unverified. **Per-ontology license checking is a Guardrail enforced in the Inventory phase regardless of provider** — do not assume blanket CC-BY (Pass 1, A10 — falsified; OBO Foundry registry confirms the spread: CC-BY 4.0, CC-BY 3.0, CC0, Apache 2.0, GPL-3.0, Artistic-2.0 all appear).

2. **Elicit MCP integration is out of scope for this skill family**: Elicit is a _source_ ontology (evidence supplier), never a _constraint_ ontology (Pass 1, §A.2; Pass 2, §6). If a future Elicit MCP integration is proposed, it feeds GSR's substrate (adds evidence ontologies to the registry), not CFR's constraint role. A separate skill (`elicit-evidence-supplier`) would be the right home, not GSR or CFR.
3. **The forcing operator is the load-bearing mechanism**: CFR's manifest description and the `cfr-project.j2` template contract must specify minimal-satisfiability projection as the forcing operator. If a future refactor changes the operator (e.g., to entailment or bare satisfiability), the three-criterion test's discriminating power changes — this is a breaking change, not a refactor.
4. **The weakened thesis T1' must not drift back to T1**: CFR's manifest description explicitly says "a mechanism, not the only one" and distinguishes CFR from retrieval-and-grounding (Elicit) and analogy. If a future PR claims CFR is "the mechanism of interdisciplinary generativity," that's a regression to the falsified-universality claim.
5. **The Phase F evolution is illustrative, not statistically significant**: the evolution in Pass 2 used a single rater (the author). A multi-rater empirical study would strengthen M1's corroboration. The skill family does not depend on the evolution's statistical power — the framework's acceptance criterion (a second agent can instantiate it) was tested by the worked example, not by the evolution.
6. **M3 (shared substrate) is not directly tested**: the Phase F evolution did not manipulate shared upper ontologies. Weak evidence against M3 (FIBO→ESO with no shared upper ontology produced a high-novelty mutant), but a dedicated test (T-substrate-alignment from Pass 1 §C.4) is deferred to future work.
7. **The Kauffman adjacent-possible anchor remains Hypothesis-tier**: arXiv:2607.12736 was mis-cited (it's the Aïra paper, not Kauffman). The Kauffman substrate (Kauffman 1995, 2000) was not verified against a primary source in this mission. The skill family does not depend on Kauffman — the forcing operator (minimal-satisfiability projection) is grounded in model theory and OWL reasoning, not in Kauffman's adjacent-possible theory. If a future session verifies Kauffman, it can be added as a substrate ontology reference; until then, it is not an anchor.
8. **Suggested .rules additions** (per the project's rules-hygiene section — the operator decides what gets merged):
   - **Cross-ontology skill substrate ≠ codebase substrate**: when a skill's substrate is ontology namespaces (not code artifacts), gradient-hunter's codebase/telemetry field taxonomy does not directly apply — the skill needs an Inventory phase (enumerate ontologies) and a Select-seeds phase (pick most-central concept) that gradient-hunter lacks. GSR is not a re-skin of gradient-hunter; the substrate difference requires new phases. (Non-obvious: an agent might assume gradient-hunter's taxonomy port directly to ontologies. It doesn't — the substrate is different.)
   - **Forcing-operator specification is load-bearing**: a constraint-forces recast skill must specify the forcing operator (minimal-satisfiability projection, entailment, or bare satisfiability) in its manifest description. The operator determines the three-criterion test's discriminating power. Changing the operator is a breaking change, not a refactor. (Non-obvious: an agent might treat the operator as an implementation detail. It's not — it's the mechanism.)
   - **Ontology-source provider abstraction, not hard-coded BioPortal**: GSR's external ontology substrate is a provider abstraction over OBO Foundry (primary, no auth, direct OWL, ~200+ ontologies), OntoBee (SPARQL mirror), and BioPortal (aggregator, requires apikey, 1,288 ontologies). OBO Foundry is the canonical source — BioPortal and OntoBee mirror it. Hard-coding BioPortal as the only extension path couples the skill to one aggregator's auth and rate limits. The provider abstraction also makes per-ontology license checking uniform (OBO Foundry registry YAML exposes the license per acronym; BioPortal exposes `hasLicense` per submission — both per-ontology, never blanket). (Non-obvious: an agent might assume OBO Foundry = CC-BY. The registry confirms a real spread — CC-BY 4.0, CC-BY 3.0, CC0, Apache 2.0, GPL-3.0, Artistic-2.0 — so each ontology must be checked.)
