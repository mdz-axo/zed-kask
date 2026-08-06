# Interdisciplinary Reasoning as a Constraint-Forces Exercise: Research Report

**Status**: Pass 1 of 3 (report → frameworks → skill spec). This document is the research report only. Frameworks and skill specs are deferred to subsequent passes per the mission's sequencing constraint.

**Date**: 2026-08-06

**Thesis under test (T1)**: The generative power of interdisciplinary research comes from recasting a concept from ontology A into the constraints and context of ontology B, forcing the concept to mutate. This is a constraint-forces exercise; the "adjacent possible" is the set of mutants that survive B's constraints.

**Falsifier (stated up front)**: T1 is wrong if recasting produces no mutant that is simultaneously (i) expressible in A's vocabulary, (ii) not already present in A, and (iii) internally consistent under B's axioms. If recasting only yields analogy or paraphrase with no constraint-driven mutation, then "generative" is metaphor, not mechanism, and the frameworks must say so.

---

## Phase A — Ground (pragmatic-semantics)

### A.0 Anchor verification outcomes (before classification)

Three anchors were designated Hypothesis-tier by the mission brief and required verification before reliance. Two failed verification. This is itself the first finding.

| Anchor                        | Brief's claim                                            | Verified reality                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | Verdict                                                                                                                                                                                                                                          |
| ----------------------------- | -------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| arXiv:2607.12736              | "Kauffman adjacent possible / Darwinian pre-affordances" | "Aïra: Rethinking AI Research Assistants for Interdisciplinary Science" (Mirji et al., 2026, cs.HC/cs.MA) — an HCI paper about an AI research assistant for interdisciplinary _teams_, not Kauffman's adjacent-possible theory                                                                                                                                                                                                                                                                              | **Mis-cited. Downgraded to one data point on interdisciplinary tooling, not a theoretical anchor.** The Kauffman adjacent-possible substrate (Kauffman 1995, 2000; Longo et al. 2012) is not verified by this fetch and remains Hypothesis-tier. |
| BioPortal free API            | "IS but unverified"                                      | Verified: `data.bioontology.org` REST API is live, documented, returns JSON-LD. **Requires an `apikey`** (free registration via NCBO account). `/ontologies/{acronym}/download` endpoint exists with `download_format={csv\|rdf}`. License/redistribution terms per-ontology (each submission carries `hasLicense`, `useGuidelines`, `morePermissions`); OBO Foundry ontologies are generally CC-BY but this must be checked per-acronym. Rate limits not documented on the API page — treat as unverified. | **Verified as accessible with registration. Redistribution rights are per-ontology, not blanket. Scope every dependency to "BioPortal-or-equivalent OBO/OWL source, per-ontology license check required."**                                      |
| "9 ontologies in the project" | Substrate cardinality = 9                                | Verified against `kask/registry/templates/create-skill/create-skill-ontologies.yaml` and `hkask-bridge-ontology/src/axis.rs`: the project carries **6 domain-supplement namespaces** (FIBO, ESO, GOLEM, ML-Schema, OMC, SUMO) + **2 universal axes** (PKO process, DC+BIBO state) + **5W1H core** = **9 anchoring surfaces**, but only 6 are "ontologies" in the OWL/OBO sense. The create-skill registry lists 5 named ontologies + 1 `domain_specific` slot.                                              | **Partially verified. The count "9" conflates axes, namespaces, and core. The accurate substrate is: 6 domain-supplement ontology namespaces + 2 universal axes + 1 5W1H core. Use this, not "9 ontologies."**                                   |

**Implication for the thesis**: The mission's own anchor set demonstrates the failure mode the thesis predicts — a concept ("Kauffman adjacent possible") was recast into a constraint context (an arXiv ID lookup) and the recast did _not_ survive verification. The mechanism the thesis describes is exactly what caught the mis-citation. This is corroborating (not confirming) anecdotal evidence, recorded as such.

### A.1 Provenance table — every anchor claim classified

Classification axes per pragmatic-semantics: ontological mode (IS/OUGHT), epistemic mode (declarative/probabilistic/subjunctive), constraint force (Prohibition/Guardrail/Guideline/Evidence/Hypothesis), provenance, ontology anchoring tier, confidence.

| #   | Claim                                                                                                                 | IS/OUGHT | Epistemic     | Constraint force | Provenance                                                                  | Ontology tier                           | Confidence                                                | Notes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| --- | --------------------------------------------------------------------------------------------------------------------- | -------- | ------------- | ---------------- | --------------------------------------------------------------------------- | --------------------------------------- | --------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A1  | Elicit is a specialized research assistant                                                                            | IS       | Declarative   | Evidence         | External (elicit.com, fetched 2026-08-06)                                   | Domain supplement (no project ontology) | 0.85                                                      | Marketing page; treat as vendor self-description                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| A2  | Elicit operationalizes "good reasoning" via systematic-review-inspired process + sentence-level citations             | IS       | Declarative   | Evidence         | External (elicit.com features + "How we're different")                      | Domain supplement                       | 0.70                                                      | Inferred from marketing copy; not a peer-reviewed claim                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| A3  | Elicit's tool surface: Research agent, Search (138M papers, 545K trials), Reports, Systematic Review, Library, Alerts | IS       | Declarative   | Evidence         | External                                                                    | Domain supplement                       | 0.80                                                      | Verbatim from features page                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| A4  | Elicit MCP server sits alongside the Elicit API as an autonomous-research surface                                     | IS       | Declarative   | Evidence         | External (blog post title "The Elicit API and MCP", Jul 15 2026)            | Domain supplement                       | 0.75                                                      | Title-level only; not deeply verified                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| A5  | arXiv:2607.12736 is Kauffman adjacent-possible theory                                                                 | IS       | Declarative   | (was Hypothesis) | External (arXiv fetch)                                                      | —                                       | **0.00 — FALSE**                                          | Falsified by direct fetch. Paper is Aïra, an HCI paper. The citation does not point at a Kauffman paper — but this does NOT falsify the Kauffman pattern (A6). The citation and the pattern are different layers (see translational amendment §4.4).                                                                                                                                                                                                                                                                                                                   |
| A6  | Kauffman adjacent possible: the set of states reachable by one mutation from the current state                        | IS       | Probabilistic | Hypothesis       | External (YouTube transcript `nEtATZePGmg` via SerpAPI, fetched 2026-08-06) | Domain supplement                       | 0.65 — **admitted (provenance + admissibility verified)** | Central claim: the biosphere innovates by combinatorially recombining existing things, producing a 'delay-and-burst' (hockey-stick) trajectory whose future contents cannot be deduced from present state. Falsifier: a combinatorial-innovation domain where the substrate grows but the rate of novelty production does NOT accelerate (inter-novelty waiting time fails to halve per doubling of substrate). Admitted but empirically untested — awaits discriminating trajectory test against a null model. ASR transcript (near-verbatim, not publication-grade). |
| A7  | BioPortal is free and open to all users, no login required for browsing                                               | IS       | Declarative   | Evidence         | External (bioportal.bioontology.org footer)                                 | Domain supplement                       | 0.90                                                      | Browsing is login-free; API requires apikey                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| A8  | BioPortal API requires an apikey (free registration)                                                                  | IS       | Declarative   | Evidence         | External (data.bioontology.org/documentation)                               | Domain supplement                       | 0.95                                                      | Verbatim from API docs                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| A9  | BioPortal permits OBO/OWL download via `/ontologies/{acronym}/download?download_format=rdf`                           | IS       | Declarative   | Evidence         | External (API docs: Ontology resource, GET download)                        | Domain supplement                       | 0.85                                                      | Endpoint exists; per-ontology license governs redistribution                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| A10 | BioPortal redistribution rights are blanket-free                                                                      | IS       | Declarative   | (was Hypothesis) | —                                                                           | —                                       | **0.10 — likely FALSE**                                   | Each submission carries `hasLicense`/`useGuidelines`/`morePermissions`; rights are per-ontology. Do not assume blanket CC-BY.                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| A11 | The project has 9 ontologies                                                                                          | IS       | Declarative   | (was Hypothesis) | Internal (registry + axis.rs)                                               | Core                                    | **0.40 — imprecise**                                      | 6 domain-supplement namespaces + 2 axes + 1 core. "9 ontologies" conflates categories.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| A12 | T1: interdisciplinary generativity = constraint-forces recasting                                                      | IS       | Subjunctive   | Hypothesis       | Inference (mission brief)                                                   | Domain supplement                       | 0.45                                                      | The thesis itself; to be falsified in Phase C                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| A13 | The mission's sequencing constraint (report → frameworks → spec, no bundling)                                         | OUGHT    | Declarative   | Prohibition      | Specification (mission brief)                                               | Core                                    | 1.00                                                      | Governs this document's scope                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |

**Convergence signal for Phase A**: zero unclassified claims. ✓ Achieved — all 13 claims classified. Two claims (A5, A10) were falsified during classification; one (A11) was corrected; one (A6) remains Hypothesis-tier and is quarantined.

### A.2 Elicit extraction (narrow, per mission constraint)

The mission permits extracting only three things from Elicit. Treating Elicit as one IS/Evidence data point, not the target framing.

**(a) How Elicit operationalizes "good reasoning"** — Elicit's self-described reasoning model is _systematic-review-inspired process discipline_ + _sentence-level citation transparency_. From the marketing copy: reports are "based on a process inspired by systematic reviews"; "Elicit supports all AI-generated claims with sentence-level citations from the underlying sources." The implicit theory of good reasoning is _traceability to primary sources + structured extraction over free generation_. This is a retrieval-and-grounding theory, not a constraint-forces theory. Elicit does not claim to mutate concepts across disciplinary ontologies; it claims to surface and aggregate evidence within a discipline. **Elicit is therefore a negative data point for T1**: a successful interdisciplinary tool whose theory of reasoning does _not_ involve constraint-forces recasting. This must be explained by T1 or T1 is incomplete.

**(b) Elicit's tool surface** — Research agent (multi-source gathering + cited artifacts), Search (138M papers, 545K clinical trials, semantic search), Reports (customizable briefs), Systematic Literature Review (PRISMA 2020, screening + extraction), Library, Alerts. The surface is _retrieval-centric_: every tool is a variant of "find and structure existing knowledge." None of the tools recast a concept from one ontology into another.

**(c) Where the MCP server sits** — Alongside the Elicit API (announced Jul 15, 2026), as an autonomous-research surface: "bring Elicit's trusted capabilities to your agents and workflows." The MCP server exposes the retrieval-centric surface to external agents. It is _not_ positioned as a reasoning engine; it is positioned as a trusted evidence-fetcher. **Implication for the skill spec (Pass 3)**: an Elicit MCP integration would be a _source_ ontology (evidence supplier), never a _constraint_ ontology. The constraint-forces loop needs a constraint source; Elicit cannot play that role.

---

## Phase B — Frame (hypothesis-framer)

### B.1 FINER evaluation of T1

| Dimension   | Score (0–10) | Rationale                                                                                                                                                                                        |
| ----------- | ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Feasible    | 7            | Testable on a small concept corpus with the project's existing ontology registry. Does not require BioPortal (project ontologies suffice for a first test).                                      |
| Interesting | 9            | If true, provides a mechanism for interdisciplinary generativity that is operationalizable as a skill family. If false, the negative result is itself valuable (rules out a seductive metaphor). |
| Novel       | 8            | The constraint-forces framing of interdisciplinarity is not the standard framing (which is analogy/transfer/metaphor). The falsifier is novel.                                                   |
| Ethical     | 10           | No human subjects, no sensitive data. Concept recasting is low-risk.                                                                                                                             |
| Relevant    | 8            | Directly relevant to the skill family the mission asks for.                                                                                                                                      |

Lowest dimension: Feasibility (7). Refinement: bound the test to the project's 6 domain-supplement ontologies to avoid BioPortal dependency in the first test.

### B.2 Refined research question (PICO-structured)

- **Population**: concepts drawn from the project's 6 domain-supplement ontology namespaces (FIBO, ESO, GOLEM, ML-Schema, OMC, SUMO), one concept per source ontology.
- **Intervention**: recasting each concept into the constraint context of a _different_ target ontology (e.g., a FIBO concept recast under ESO's event/situation/role axioms).
- **Comparison**: the same concept subjected to _paraphrase only_ (restatement in the source ontology's own vocabulary, no constraint context switch).
- **Outcome**: a binary discriminating outcome — does the recast produce a mutant satisfying the three falsifier criteria (expressible in A's vocabulary, not present in A, consistent under B's axioms)?

**Structured question**: "Among concepts drawn from the project's six domain-supplement ontologies, does recasting a concept into a different ontology's constraint context produce a mutant that is expressible in the source vocabulary, absent from the source, and consistent under the target's axioms, more often than paraphrase alone?"

### B.3 Hypotheses with nulls

**H1 (the thesis, directional)**: Recasting a concept from ontology A into ontology B's constraint context produces mutants satisfying all three falsifier criteria (expressible-in-A, absent-from-A, consistent-under-B) at a rate strictly greater than paraphrase alone.

**H0 (null for H1)**: There is no difference in the rate of three-criterion-satisfying mutants between recasting and paraphrase.

**H2 (mechanism-strength, directional)**: The mutation is _constraint-driven_ — the specific mutant form is predictable from B's axioms, not from A's structure. If H1 holds but H2 does not, recasting is generative but not via the constraint-forces mechanism T1 claims (it would be generative via some other mechanism, e.g., random perturbation).

**H0 for H2**: The mutant form is not predictable from B's axioms above chance.

**H3 (variety-gate, directional)**: Recasting is generative only when the target ontology B has _greater variety_ (in Ashby's sense) than the source ontology A along the dimension the concept occupies. If H3 holds, T1 is sharpened: the constraint-forces mechanism requires requisite variety. If H3 fails, variety is not the gate.

**H0 for H3**: Generativity is independent of the variety ratio between source and target.

**Convergence signal for Phase B**: each H has a null. ✓ Achieved — H1/H0, H2/H0, H3/H0.

---

## Phase C — Stress (falsifiability + lean-prover)

### C.1 Popper admissibility gate

- T1 is an IS-mode claim (descriptive: "generative power _comes from_ recasting"). ✓
- Epistemic mode: subjunctive (a mechanism claim). Its counterfactual is testable: "in a world identical to ours except that recasting did not occur, would the mutant still be produced?" ✓
- Concrete falsifying observation: recasting yields only mutants that fail at least one of (i) expressible-in-A, (ii) absent-from-A, (iii) consistent-under-B. ✓
- **Verdict: ADMIT.** T1 is testable.

### C.2 Multiple working hypotheses (Chamberlin/Platt) — for _why_ recasting might or might not be generative

| ID  | Hypothesis                                                                                                         | Prediction                                                                                                   | Falsifier                                                                             | Diversity role                                                                                     |
| --- | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| M1  | Recasting is generative via constraint forces (T1 as stated)                                                       | Mutant form is predictable from B's axioms; paraphrase controls produce fewer three-criterion mutants        | Mutant form is _not_ predictable from B's axioms, OR paraphrase produces equally many | The obvious explanation                                                                            |
| M2  | Recasting is generative via _random perturbation_, not constraint                                                  | Mutant form is unpredictable from B's axioms; recasting and random-perturbation controls produce equal rates | Mutant form _is_ predictable from B's axioms                                          | Challenges the obvious                                                                             |
| M3  | Recasting is generative only when A and B share a common substrate ontology (e.g., both inherit from DOLCE or BFO) | Three-criterion mutants appear only for ontology pairs with a shared upper ontology                          | Three-criterion mutants appear for pairs with no shared upper ontology                | Embarrassing-if-true (would mean T1 is really "upper-ontology inheritance," not constraint forces) |
| M4  | Recasting is _not_ generative — what looks like mutation is post-hoc rationalization                               | Independent raters cannot distinguish "mutant" from "paraphrase with relabeled vocabulary" above chance      | Independent raters _can_ distinguish above chance                                     | Unlikely / null-ish                                                                                |

### C.3 Counterfactuals (Pearl do-operator)

For M1 vs M2 (the discriminating pair):

- **do(not recast)**: hold the concept fixed in A, do not expose to B's constraints. Apply a _vocabularly swap_ (replace A's terms with B's terms without applying B's axioms). This is the paraphrase-with-relabel control.
- **Testable consequence**: if M1 holds, the recast mutant's _structural form_ (e.g., a FIBO "Instrument" recast under ESO becomes an "Event with a Role" — a category change, not a relabel) differs from the relabel control's form (a FIBO "Instrument" relabeled as ESO "Entity" — same category, new word). If M2 holds, both produce the same distribution of forms.

For M3:

- **do(force shared substrate)**: align A and B to a common upper ontology (BFO or DOLCE) before recasting. If M3 holds, alignment increases the three-criterion mutant rate. Natural experiment: the project's 6 ontologies vary in upper-ontology inheritance (FIBO inherits from FIBO-DSL/SUMO; GOLEM does not declare an upper ontology) — this variation is a natural experiment.

### C.4 Discriminating tests (Platt) — coverage matrix

| Test \ Hypothesis                                                                                                                                                         | M1 (constraint)                                     | M2 (random)                                    | M3 (shared substrate)                               | M4 (rationalization)                             |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- | ---------------------------------------------- | --------------------------------------------------- | ------------------------------------------------ |
| **T-recast-vs-relabel**: recast 6 concepts (one per source ontology) into a target ontology; relabel-control 6 more. Blind-rater judges "category change" vs "word swap." | corroborates if recast > relabel on category change | falsifies if recast = random-perturbation rate | neutral                                             | corroborates if raters can't distinguish         |
| **T-axiom-predictability**: for each recast mutant, ask "which of B's axioms forced this form?" If ratable, mutant is constraint-driven.                                  | corroborates if axiom-identifiable                  | falsifies if not                               | neutral                                             | neutral                                          |
| **T-substrate-alignment**: recast pairs with and without shared upper ontology; compare three-criterion rates.                                                            | neutral                                             | neutral                                        | corroborates if shared-substrate pairs > non-shared | neutral                                          |
| **T-blind-discrimination**: independent raters classify outputs as "mutant" vs "paraphrase" without knowing which procedure produced them.                                | neutral                                             | neutral                                        | neutral                                             | falsifies if raters can distinguish above chance |

Every hypothesis is falsifiable by at least one test. ✓ No irreducible pairs (M1 and M2 predict different outcomes on T-recast-vs-relabel and T-axiom-predictability).

### C.5 At least one runnable test (convergence requirement)

**T-recast-vs-relabel is runnable as-is** on the project's existing ontology registry, no BioPortal dependency:

1. Select 6 seed concepts, one per source ontology:
   - FIBO: `FinancialInstrument`
   - ESO: `Event`
   - GOLEM: `Character`
   - ML-Schema: `Model`
   - OMC: `CreativeWork`
   - SUMO: `Process`
2. For each, run two procedures:
   - **Recast**: re-express the concept under a _different_ target ontology's axioms (e.g., FIBO `FinancialInstrument` → ESO: an instrument is an Event with pre/post situations and participant Roles, not a static entity). Record the resulting form.
   - **Relabel-control**: swap the concept's vocabulary to the target ontology's terms without applying its axioms (e.g., FIBO `FinancialInstrument` → ESO `Entity` — same category, new word). Record the resulting form.
3. Blind-rater test: present the 12 outputs (6 recast + 6 relabel, shuffled) to an independent rater who classifies each as "category change" or "word swap only."
4. Discriminating outcome: if recast outputs are classified as "category change" at a rate > relabel outputs (binomial test, α = 0.05), M1 is corroborated and M2/M4 are falsified. If rates are equal, M1 is falsified.

**Worked example (one cell, to show the test is concrete)**:

- Source: FIBO `FinancialInstrument` — a static class denoting a contract that has financial value.
- Target: ESO axioms — every entity is situated in a pre-event Situation and a post-event Situation, with Roles filled by participants.
- **Recast mutant**: "A financial instrument is an Event (the issuance) whose pre-situation is a commitment-of-capital and whose post-situation is a cash-flow-pattern, with the roles of issuer, holder, and obligor." — This is (i) expressible in FIBO vocabulary (instrument, issuance, cash flow), (ii) not present in FIBO (FIBO models instruments as static entities, not as events with situations), (iii) consistent under ESO axioms (it uses Event/Situation/Role correctly). **Three-criterion mutant. ✓**
- **Relabel control**: "A financial instrument is an ESO Entity." — (i) expressible in FIBO, (ii) not present in FIBO (ESO Entity is not a FIBO term), (iii) consistent under ESO (Entity is the root). But this is a _relabel_, not a mutation — the concept's structure is unchanged. A blind rater should classify this as "word swap only."

This single cell illustrates the discriminating signal. The full test runs 6 cells × 2 procedures.

### C.6 Lean obligation sketch (where formal encoding is possible)

The three-criterion mutant definition is partially formalizable. Let `A` be the source ontology (a theory in a logic), `B` the target ontology, `c` a concept, `mutant(c, A, B)` the recast result.

```lean
-- Three-criterion mutant (sketch, not compiled)
structure ThreeCriterionMutant (A B : Theory) (c : A.Concept) :=
  -- (i) expressible in A's vocabulary
  expressible_in_A : ∃ (t : A.Term), denotes t mutant
  -- (ii) not already present in A
  absent_from_A : ¬ ∃ (d : A.Concept), equivalent d mutant
  -- (iii) internally consistent under B's axioms
  consistent_under_B : B ⊢ mutant.well_formed
```

The full Lean obligation (proving that a _specific_ recast satisfies this structure for a specific A, B, c) requires formal encodings of FIBO and ESO that do not exist in machine-checkable form in this project. **The sketch is therefore a specification of the proof obligation, not a runnable proof.** This is honest: the convergence requirement is "≥1 test runnable as-is," satisfied by T-recast-vs-relabel (a human-rater test, not a machine proof). The Lean sketch is for the frameworks pass, where it specifies what a future formal-verification skill would need to discharge.

**Convergence signal for Phase C**: ≥1 test runnable as-is. ✓ Achieved — T-recast-vs-relabel.

---

## Phase D — Map substrate (gradient-hunter)

### D.1 Substrate inventory (verified, not assumed)

From `hkask-bridge-ontology/src/axis.rs` and `create-skill-ontologies.yaml`:

| Layer                              | Members                                                         | Role                                                                                                                       |
| ---------------------------------- | --------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| Core (5W1H)                        | Who/What/When/Where/Why/How                                     | Universal ground; no domain supplement                                                                                     |
| Universal axes (2)                 | PKO (process), DC+BIBO (state)                                  | Dual-axis framework applied to every artifact                                                                              |
| Domain-supplement namespaces (6)   | FIBO, ESO, GOLEM, ML-Schema, OMC, SUMO                          | Domain-specific precision layered on the dual-axis core                                                                    |
| create-skill registry (5 + 1 slot) | PKO, Dublin Core, GOLEM, MovieLabs OMC, ESO + `domain_specific` | The skill-creation ontology reference set (overlaps with the 6 namespaces; PKO and DC are universal axes, not supplements) |

**BioPortal-or-equivalent candidates** (for extending the substrate beyond the project's 6): OBO Foundry ontologies (GO, CHEBI, SNOMED CT, NCIT, etc.), accessible via `/ontologies/{acronym}/download?download_format=rdf` with per-ontology license. **Flagged assumption**: BioPortal redistribution rights are per-ontology; the skill spec must check `hasLicense` per acronym before caching OWL locally.

### D.2 Gradient map — steep gradients as recombination sites

Applying gradient-hunter's Prior → Map → Detect with the prior = "every domain-supplement namespace should have a populated recombination surface with every other namespace." The prior is the complete graph K₆ on the 6 namespaces. The actual field is sparse.

| Site   | Source ontology   | Target ontology               | Gradient shape    | Populated side                                                                                                | Unpopulated side                                                                                                                                                                                                                              | Reason hypothesis                                                                                                                |
| ------ | ----------------- | ----------------------------- | ----------------- | ------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| **G1** | FIBO (financial)  | ESO (events)                  | Sharp cliff       | FIBO has rich static-class hierarchy (instruments, products, markets)                                         | No FIBO concept recast as an ESO event-with-situations (the worked example in C.5 is the first instance)                                                                                                                                      | MCAR — the recombination was never attempted, not blocked                                                                        |
| **G2** | GOLEM (narrative) | ML-Schema (ML experiments)    | Roof edge         | GOLEM models narrative structure (character, event, plot); ML-Schema models experiments (model, dataset, run) | No GOLEM concept recast under ML-Schema's experimental-reproducibility axioms (e.g., a "Character" as a Model with a training Run and evaluation Metrics — a narrative-as-experiment mutant)                                                  | MAR — explainable gap; the two domains seem unrelated, but the gradient's steepness is the signal                                |
| **G3** | SUMO (general)    | OMC (media)                   | Topological hole  | SUMO has a `Process` concept; OMC has a `Workflow` concept with pipeline stages                               | No SUMO `Process` recast under OMC's pipeline-stage axioms (a Process as a Capture→Post→Distribute pipeline)                                                                                                                                  | MNAR — the mapping exists conceptually but was never wired                                                                       |
| **G4** | ESO (events)      | FIBO (financial)              | (reverse of G1)   | ESO events have pre/post situations                                                                           | No ESO `Event` recast as a FIBO `FinancialInstrument` (an event as a tradable contract)                                                                                                                                                       | MCAR — never attempted                                                                                                           |
| **G5** | ML-Schema         | GOLEM                         | (reverse of G2)   | ML-Schema models have training runs                                                                           | No ML-Schema `Model` recast as a GOLEM `Character` (a model as a narrative character with a character arc = training curve)                                                                                                                   | MAR — explainable but generative (the mutant "model-as-character" is a known trope in ML interpretability circles, unformalized) |
| **G6** | OMC (media)       | PKO (process, universal axis) | Wombling boundary | OMC workflows have pipeline stages; PKO procedures have steps with executions                                 | The boundary is _between_ a domain supplement and a universal axis — recasting OMC into PKO should be trivial (both are process ontologies), but the mutation would be small. **This is a low-gradient site — useful as a negative control.** | Intentional boundary — the two are already aligned by design                                                                     |

**Convergence signal for Phase D**: gradient map names ≥3 sites. ✓ Achieved — 6 sites named, 5 high-gradient (G1–G5), 1 low-gradient negative control (G6).

### D.3 Recombination-site priority (gradient-hunter priority ordering)

Priority: broken allosteric coupling > metastable trap > MNAR > MAR > MCAR, then fractal recurrence, then magnitude, then populated-side criticality.

- **G3 (MNAR)** is highest priority — the mapping exists conceptually but was never wired; the mutant is "forgotten," not "absent."
- **G1, G4 (MCAR)** are lowest priority by reason class, but highest by _fractal recurrence_: the FIBO↔ESO recombination recurs at multiple scales (instrument-as-event, market-as-event, trade-as-event). Fractal recurrence elevates priority.
- **G2, G5 (MAR)** are medium — explainable gaps, but the "narrative-as-experiment" / "model-as-character" mutants have known cultural analogues (interpretability personas, narrative-driven ML), suggesting the gradient is real and the recombination would be generative.

---

## Phase E — Cybernetic (pragmatic-cybernetics)

### E.1 The recombination loop

```
   ┌─────────────────────────────────────────────────────────────┐
   │                                                             ▼
[Seed concept c in A]──▶[Recast into B's axioms]──▶[Mutant m]──▶[Three-criterion test]
   ▲                                                       │            │
   │                                                       │            │ verdict
   │                                                       ▼            │
   │                                              [Pareto frontier]◀──┘
   │                                              (novelty, validity)
   │                                                       │
   │                                                       ▼
   └──────────────[next seed from gradient map]◀──────────[feedback]
```

### E.2 Five-property loop analysis

| Property | Assessment                  | Evidence                                                                                                                                                                                                                 |
| -------- | --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Polarity | Healthy (negative feedback) | A failed three-criterion test reduces the next seed's likelihood of being drawn from the same source-target pair; the loop corrects toward generative pairs                                                              |
| Delay    | Degraded (long)             | The delay between recast and verdict requires a human rater (T-recast-vs-relabel) or a formal proof (Lean sketch, not yet runnable). Delay is bounded by rater availability, not by the loop itself                      |
| Gain     | Healthy                     | Each iteration produces exactly one mutant per seed; gain is unitary and stable                                                                                                                                          |
| Closure  | Healthy                     | The verdict feeds back into the gradient map (Phase D) and the seed selection (next iteration's Prior)                                                                                                                   |
| Fidelity | Degraded                    | The three-criterion test's "expressible in A" and "absent from A" criteria are rater-subjective; "consistent under B" is partially objective (B's axioms are checkable) but depends on the axiom encoding's completeness |

**Diagnosis**: the loop is _not broken_ but has two degraded properties (delay, fidelity). Remediation: (a) for delay — automate the "consistent under B" check via an OWL reasoner (Hermit, Pellet) once OWL encodings are available; (b) for fidelity — operationalize "expressible in A" as "the mutant's terms are all in A's term list" (mechanical) and "absent from A" as "the mutant's structure is not subsumed by any A concept" (mechanical via a subsumption reasoner).

### E.3 Ashby requisite variety audit

- **Disturbance classes** (the variety the system — interdisciplinary concept space — can produce): the 6×5 = 30 ordered source-target pairs, each yielding a potentially infinite set of mutants. System variety is very high (effectively unbounded).
- **Response classes** (the variety the regulator — the three-criterion test — can produce): {pass, fail} per criterion × 3 criteria = 8 verdict classes. Plus the Pareto-frontier ranking (continuous).
- **Deficit**: regulator variety (8 + continuous) << system variety (unbounded). **Ashby's Law is violated.** The regulator cannot distinguish all mutants the system can produce.
- **Attenuation strategy**: restrict the source-target pairs to the 5 high-gradient sites from Phase D (reduces system variety from 30 to 5). This is the gradient-hunter prior — it attenuates the field to where the signal is.
- **Amplification strategy**: add a fourth criterion — "the mutant is _useful_" (e.g., it suggests a new skill, a new research question, or a new artifact). This amplifies regulator variety from 8 to 16, at the cost of introducing a subjective criterion. **Flag**: "useful" is an OUGHT criterion; per pragmatic-semantics, OUGHT does not override IS in a descriptive test. Keep "useful" as a _secondary_ outcome, not a primary criterion.

### E.4 Good Regulator check

The Conant-Ashby Good Regulator theorem requires that the regulator _model_ the system it regulates. The three-criterion test models the system as "a concept plus a target ontology's axioms." This model is adequate _if_ the target ontology's axioms are faithfully encoded. The model is _inadequate_ if the target ontology is represented only by its term list (as in the relabel control) — a term list is not a model of the ontology's constraint structure. **Implication**: the recast procedure must apply B's _axioms_, not just B's _vocabulary_. This is the operational distinction between M1 (constraint forces) and M2 (random perturbation / relabel).

**Convergence signal for Phase E**: loop diagram + variety audit. ✓ Achieved.

---

## Phase F — Evolve (gpa-evolution, textual-gradient)

Phase F requires running textual-gradient evolution over a small set of concept-recast artifacts and keeping the Pareto frontier on (novelty, validity, cost) stable for ≥2 iterations. This is a generative step that, per the mission's sequencing constraint, belongs to the _frameworks_ pass (Pass 2), not the report pass (Pass 1). The report pass establishes the _test_ (Phase C) and the _substrate_ (Phase D) that the evolution will run over.

**What the report pass contributes to Phase F**:

- The seed set: 6 concepts, one per source ontology (C.5).
- The mutation operator: recast-into-B's-axioms (E.4).
- The fitness dimensions: novelty (= absent-from-A), validity (= consistent-under-B), cost (= rater/proof effort).
- The convergence criterion: Pareto frontier stable ≥2 iterations.

**What is deferred to Pass 2**: actually running the evolution and reporting the frontier. This deferral is consistent with the mission's "one primary deliverable per pass" constraint. The report's acceptance criterion (a skeptic can run one test from C and get a discriminating result) does not require the evolution to have run.

**Honest note**: deferring Phase F's execution to Pass 2 means the report's acceptance criterion rests entirely on Phase C's T-recast-vs-relabel. This is acceptable — the test is runnable as-is and discriminating — but it means the report _corroborates_ T1 only via the single worked example (C.5), not via a run. The frameworks pass will run the evolution.

---

## Phase G — Self-watch (metacognition)

Brier-scored predictions per phase. Brier score for a binary prediction: `(predicted_probability - actual_outcome)²`, where actual_outcome ∈ {0, 1}.

| Phase | Prediction                              | p (predicted)                                                       | Outcome                 | Brier  |
| ----- | --------------------------------------- | ------------------------------------------------------------------- | ----------------------- | ------ |
| A     | "All anchors will verify as stated"     | 0.50 (Hypothesis-tier by mission design)                            | 0 (2 of 3 failed)       | 0.25   |
| B     | "T1 will admit through the Popper gate" | 0.85                                                                | 1 (admitted)            | 0.0225 |
| C     | "≥1 test will be runnable as-is"        | 0.80                                                                | 1 (T-recast-vs-relabel) | 0.04   |
| D     | "Gradient map will name ≥3 sites"       | 0.90                                                                | 1 (6 sites)             | 0.01   |
| E     | "Ashby variety will be satisfied"       | 0.40 (cybernetics prior: regulators usually lack requisite variety) | 0 (violated)            | 0.16   |
| F     | "Evolution will run in this pass"       | 0.10 (sequencing constraint makes this unlikely)                    | 0 (deferred)            | 0.01   |

**Calibration log summary**: mean Brier = 0.082. The worst-calibrated prediction was Phase A (Brier 0.25) — I under-weighted how many anchors would fail. The best-calibrated was Phase F (Brier 0.01) — I correctly predicted the deferral. **Lesson for Pass 2**: when a mission designates anchors as Hypothesis-tier, predict a higher failure rate (p ≈ 0.30 for "all verify," not 0.50).

**Convergence signal for Phase G**: log entry per phase. ✓ Achieved — 6 entries.

---

## Phase H — Critic (grill-me, decoupled)

**Decoupling statement**: Phase H is a separate pass from the generator (phases A–G). The generator was the report author; the critic is invoked as an independent interrogation. To enforce decoupling in a single-agent context, the critic reads only the report's _outputs_ (the provenance table, the hypothesis set, the test design, the gradient map, the loop diagram), not the generator's reasoning trace. The critic escalates Recall → Mechanism → Rationale → Edge → Synthesis.

### H.1 Recall

- **Q**: State the three falsifier criteria for T1.
- **A** (from report): (i) expressible in A's vocabulary, (ii) not already present in A, (iii) internally consistent under B's axioms. ✓

- **Q**: Which two anchors failed verification in Phase A?
- **A**: arXiv:2607.12736 (mis-cited; is Aïra, not Kauffman) and BioPortal blanket-free redistribution (per-ontology, not blanket). ✓

### H.2 Mechanism

- **Q**: By what mechanism does recasting produce a mutant, per T1? Name the specific step where mutation occurs.
- **A** (from report): mutation occurs when the concept is forced to satisfy B's _axioms_ (not B's vocabulary). The mechanism is constraint application: B's axioms (e.g., ESO's "every entity is situated in pre/post situations with roles") force a structural transformation of A's concept (e.g., FIBO's static `FinancialInstrument` becomes an event-with-situations). The mutation is the structural delta between the original and the axiom-satisfying form.
- **Critic probe**: Is "constraint application" a mechanism or a description? A mechanism names _how_ the constraint forces the transformation. The report does not specify the forcing operator — is it logical entailment (B's axioms _entail_ the mutant form), satisfiability (the mutant is _a model_ of B's axioms that mentions A's terms), or something else?
- **Generator response (recorded, not regenerated)**: The report is ambiguous here. The frameworks pass (Pass 2) must specify the forcing operator. Candidate operators: (a) _entailment_ — B ⊢ mutant; (b) _satisfiability_ — mutant is a model of B; (c) _abduction_ — mutant is the minimal modification of c such that B's axioms are satisfied. **This is a real gap.** The critic flags it as a Should-fix for Pass 2.

### H.3 Rationale

- **Q**: Why is Elicit a _negative_ data point for T1, and what does T1 owe us if the negative data point stands?
- **A** (from report): Elicit is a successful interdisciplinary tool whose theory of reasoning is retrieval-and-grounding, not constraint-forces recasting. If T1 claims constraint-forces recasting is _the_ mechanism of interdisciplinary generativity, the existence of a successful interdisciplinary tool that does not use it falsifies the universality of T1. T1 must be _weakened_ to "constraint-forces recasting is _a_ mechanism, not the only one," or T1 must argue that Elicit is not actually generative (it aggregates, not generates).
- **Critic verdict**: The weakening is honest and necessary. T1-as-stated ("the generative power _comes from_ recasting") implies exclusivity. The report should have stated T1 as "a mechanism" not "the mechanism." **Flagged as a framing correction for the frameworks pass.**

### H.4 Edge cases

- **Q**: What happens when A and B are the same ontology (recast into self)?
- **A** (from report, inferred): The recast reduces to paraphrase; criterion (ii) "absent from A" cannot be satisfied. This is the degenerate case. The report does not explicitly handle it.
- **Critic probe**: What about when A and B are _subsets_ of each other (e.g., SUMO and FIBO, where FIBO inherits from SUMO)? The recast may produce a mutant that is "absent from FIBO" but "present in SUMO" — is that a three-criterion mutant or an inheritance artifact?
- **Generator response**: This is the M3 hypothesis (shared substrate). The report's T-substrate-alignment test addresses it. ✓ Adequately handled.

- **Q**: What if the mutant is expressible in A's vocabulary but only by _coining a new compound term_ (e.g., "event-instrument")? Is coining a new term "expressible in A's vocabulary" or "extending A's vocabulary"?
- **A** (from report): Not addressed. **Critic flags as a Should-fix for Pass 2.** The frameworks pass must operationalize "expressible in A's vocabulary" — does it permit novel compounds from A's term set, or only existing terms?

### H.5 Synthesis

- **Q**: Does the report's evidence corroborate T1, falsify it, or leave it undetermined?
- **A** (critic's independent verdict): The report provides _one_ worked example (C.5) that satisfies the three criteria. One example is anecdotal, not corroborative in Popper's sense (corroboration requires withstanding a test that could have falsified). The test T-recast-vs-relabel is designed but _not run_ on the full 6-cell set. Therefore:
  - T1 is **not falsified** by this report (no test failed).
  - T1 is **not corroborated** by this report (no test was run at scale).
  - T1 is **admissible and tested-in-design** — the test exists and is runnable.
  - The report's honest claim is: "T1 survives the admissibility gate and has a discriminating test; the test has not been run at scale in this pass."
- **Critic verdict**: **PASS with conditions.** The report meets its acceptance criterion (a skeptic can take T-recast-vs-relabel and run it on a fresh concept to get a discriminating result — the worked example shows how). The conditions for Pass 2 are:
  1. Specify the forcing operator (entailment / satisfiability / abduction).
  2. Weaken T1 from "the mechanism" to "a mechanism."
  3. Operationalize "expressible in A's vocabulary" w.r.t. novel compounds.
  4. Run T-recast-vs-relabel on the full 6-cell set (Phase F execution).

**Convergence signal for Phase H**: verdict = pass (with conditions). ✓ Achieved.

---

## Acceptance criteria check (Deliverable 1)

> The research report must contain the artifacts of A–H. Acceptance: a skeptic can take at least one test from C, run it on a fresh concept, and get a discriminating (non-ambiguous) result.

- **Artifacts A–H present**: ✓ (A provenance table, B hypothesis set, C test design + worked example, D gradient map, E loop diagram + variety audit, F deferral rationale, G calibration log, H critic verdict).
- **Skeptic-runnable test**: T-recast-vs-relabel (C.5). The worked example (FIBO `FinancialInstrument` → ESO) demonstrates the procedure end-to-end. A skeptic can take a fresh concept (e.g., GOLEM `Character` → ML-Schema) and run the same procedure: recast (a character as a Model with a training Run and evaluation Metrics) vs relabel (a character as an ML-Schema Entity), then blind-classify. The outcome is binary (category-change vs word-swap) and non-ambiguous. ✓
- **Falsifier stated up front**: ✓ (top of document).
- **No build before A–C complete**: ✓ (Phase F execution deferred; frameworks and skill spec are Pass 2 and Pass 3).
- **No anchor mutation into assertion**: ✓ (A5, A10 falsified; A6 quarantined as Hypothesis; A11 corrected).

---

## Conditions carried to Pass 2 (frameworks)

1. **Specify the forcing operator**: entailment vs satisfiability vs abduction. The framework must name which.
2. **Weaken T1**: "a mechanism of interdisciplinary generativity," not "the mechanism."
3. **Operationalize "expressible in A's vocabulary"**: permit or forbid novel compounds from A's term set.
4. **Run the evolution (Phase F)**: 6-cell × 2-procedure, Pareto frontier on (novelty, validity, cost), stable ≥2 iterations.
5. **Address Elicit as a negative data point**: the framework must either subsume Elicit's retrieval-and-grounding as a non-constraint-forces mechanism (pluralism) or argue Elicit is not generative.

## Conditions carried to Pass 3 (skill spec)

1. **Elicit MCP integration role**: source ontology (evidence supplier), never constraint ontology.
2. **BioPortal-or-equivalent dependency**: per-ontology license check; do not assume blanket CC-BY.
3. **Substrate cardinality**: 6 domain-supplement namespaces + 2 universal axes + 1 core, not "9 ontologies."
4. **OCAP/gas posture**: the recast procedure is a read-only operation on ontology terms (no side effects); gas is bounded by the number of source-target pairs evaluated. The three-criterion test is the gas-consuming step (rater/proof effort).
5. **Phase I deletion-test verdict (preview)**: each proposed skill must survive G1 (delete the skill — does complexity reappear in the caller?) and the ≤7 public surfaces rule. The skill family will be pruned in Pass 3.
