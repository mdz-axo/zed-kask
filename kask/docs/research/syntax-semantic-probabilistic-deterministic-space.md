---
title: "The Syntax–Semantic × Probabilistic–Deterministic Space: A Navigable 2×2"
audience: [researchers, architects, agents]
last_updated: 2026-10-01
version: "1.1.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [composition, trust]
---

# The Syntax–Semantic × Probabilistic–Deterministic Space: A Navigable 2×2

> **IS/OUGHT boundary.** Sections 1–5 report framing, prior art, a run empirical
> micro-test, and its outcomes. Sections 3–4 (the 2×2 model and navigation
> guide) are synthesis: a map to use, not observed behavior of zed-kask. The
> micro-test in §1.4 and the Lean pin in the capability log are the parts that
> were actually executed.
>
> **Claim labels.** Every load-bearing claim carries an id (C1–C15), a
> verification state, and a citation key resolved in **Sources**. D/P labels
> follow P8.4: `lisp_eval`/`lean_check` steps are D (deterministic oracle
> named); framing, grading, and synthesis judgments are P.

## 0. Resolution and what this report is

**The 4×4 → 2×2 resolution (user-confirmed).** The original framing of this
research said "4×4". Two binary axes — **R (representation): syntactic ↔
semantic** and **P (process): deterministic ↔ probabilistic** — cross to
yield **four regions**, and the user confirmed the 2×2 as the intended base
model [U1]. A graded refinement per axis (each axis read as a continuum rather
than a binary) is carried in §8 as an optional extension, not used here.

**Prior work built on (required).** This report stands on two on-tree reports
and states what it adds beyond them:

- `deterministic-vs-probabilistic-compute-routing.md` [R1] owns the **P-axis**:
  the routing claim (ratified as P8.4), entropy-source/sink framing, the
  correct-by-construction vs correct-in-expectation restatement (which
  resolves the temperature-0 objection), axis independence (compute ×
  reasoning), and the game axis. **This report adds the R-axis** —
  form ↔ meaning — as a co-equal dimension, joins it with the P-axis into a
  navigable 2×2, supplies region-by-region tool mapping with exemplar
  provenance, and runs an empirical micro-test of the syntactic-probabilistic
  → syntactic-deterministic seam that [R1] postulated but did not run.
- `artificial-curiosity-capability-space.md` [R2] owns capability-space
  exploration and the probe discipline — *what a designed probe elicits vs
  what default observation shows*. **This report instantiates that discipline
  on the syntax/semantics boundary**: the §1.4 battery is a designed probe,
  and the defect it caught was invisible under default observation
  (well-formed syntax, plausible logic).

## 1. Hypotheses and test outcomes

### 1.1 Falsifiable statements

**H1 (IS-mode, admitted by the falsifiability Popper gate).** For tasks whose
input is natural/messy and whose required output is a formal artifact
(regex, code, predicate, schema, query): (a) a one-shot LLM-generated
artifact has a non-zero formal defect rate when run on the actual deterministic
machine (ungated generation is not correctness-guaranteed); (b) a
deterministic gate plus bounded revision closes the defects; (c) the
deterministic-only path fails at the messy-input front half (extracting what
is *meant* from what is *said*). Functionally, the LLM supplies probabilistic
compute layered over deterministic templates, resources, and storage
(amortized inference over what was never written down as a function [S11]),
automating form- and protocol-handling and thereby exposing semantics as the
direct working material.

**H2.** The two boundaries are real but shifting and hard to perceive.
Testable IS-core: practitioners mis-route tasks across the joint space in
classifiable ways (deterministic tools where input entropy defeats
formalization; probabilistic tools ungated where correctness-by-construction
is required and cheap). The OUGHT residue — "practitioners *need* this model"
— is carried as a design judgment, not tested.

### 1.2 FINER screening (hypothesis-framer; two bounded refinement cycles)

| Dimension | H1 | H2 | Cycle-1 weak? | Refinement applied |
|---|---|---|---|---|
| Feasible | 6/10 | 5/10 | yes (both) | H1: operationalize as three instance-level predictions (P1–P3) with observables; H2: split testable IS-core from the OUGHT residue; incident census + structural checks runnable now, practitioner A/B designed-not-run |
| Interesting | 8/10 | 8/10 | no | — |
| Novel | 6/10 | 7/10 | no | Components published ([S11], [S8]); the joint-space navigation model is the on-tree novelty |
| Ethical | 9/10 | 9/10 | no | — |
| Relevant | 9/10 | 9/10 | no | — |

Convergence gate (`lisp_eval`, D): cycle 1 → `false` (one five-link
misalignment — the expert-cost objective was not measurable in-session —
plus the two weak dimensions, honestly recorded); after restating the cost
comparison as designed-not-run, cycle 2 → `true` (misalignment_count 0,
weak_finer 0, testable, admissible, feasible). Bound respected: exactly two
cycles.

### 1.3 PICO operationalization

Population: task instances with natural/messy input and formal required
output. Intervention: LLM-propose + deterministic-gate loop. Comparison:
deterministic-only pipeline (and ungated LLM output). Outcome: formal
validity rate per battery vector; iterations to verified; expert cost
(proxy only, designed-not-run). H1 in PICO form: *in messy→formal task
instances, the propose+gate pipeline achieves higher verified validity than
ungated LLM output, at lower expert cost than deterministic-only.* Null H0:
ungated one-shot output passes the battery (a gate adds nothing).

### 1.4 The discriminating test — designed, run, reported honestly

**Design.** The artifact under test is a one-shot Lisp predicate written from
a natural-language spec ("true iff a list of 1s/-1s encodes balanced
parentheses"), generated in a single pass before any battery result was
seen. The gate is a 12-vector battery written from the spec independently of
the predicate's implementation: 7 valid words, 5 invalid (including the
prefix-dip trap `(1 -1 -1 1)`, the element-domain vector `(2)`, and the
mixed-type vector `(1 "x")` isolated in a separate evaluation). The
metacognition prediction was pre-registered before the run (goal
`7b0deed5`): **0.40 that all 12 vectors pass**. Battery execution and verdict
computation are D (`lisp_eval`).

**Run 1 (first-shot artifact) — outcome: 5/12 defective, H0 eliminated.**
All 7 expected-true vectors passed; 4 expected-false vectors failed (the
predicate accepted invalid words) and the mixed-type vector crashed the
evaluation with a type error. The differential pattern isolates one root
cause: the generator encoded "false" as the empty list `()` — a private
convention this interpreter does not implement (`()` is truthy here; the
machine's false is a boolean). The one passing negative vector (`v08`,
unclosed) returns a comparison result — machine-boolean — confirming the
mechanism. **The algorithm logic was right; the syntax was flawless; the
failure was a machine-semantic convention mismatch** — the exact SP→SD seam
defect class. This is direct support for H1(a) and an adverse result for the
0.40 self-prediction.

**Run 2 (one revision from gate feedback) — outcome: 12/12 pass.** The
revision changed exactly the diagnosed cause: return machine booleans
(`(= 1 2)` as false) and guard element type (`numberp`) before any `=`
comparison. H1(b) corroborated: the loop closed in one revision.

**Verdict (falsifiability step-5 form, D): `multiple_corroborated`** — H1(a)
and H1(b) corroborated, H0 eliminated at the instance level; observations 2,
total 3, untested 0.

**Honest limits.** n=1 artifact, single artifact class; battery self-authored
(though designed before results and containing vectors targeting the
generator's genuine unknowns — the mixed-type behavior was an honest
uncertainty); H1(c) designed but not run (no deterministic-only arm
executed); the rate claim behind H1(a) needs N instances. The
"expose-semantics / work directly in meaning-space" half of H1 remains an
inference (C10) grounded in the amortized-inference mechanism [S11] and the
toolchain instances below, not in a measured cost comparison.

**Metacognition calibration.** Prediction 0.40 pass-all; outcome fail(0);
Brier = 0.16 (D, computed; operator resolution pending on goal `7b0deed5` —
this session never scores its own prediction). Notably the failure class
differed from the predicted worry (prefix-dip logic, mixed-type `=`); the
actual cause — truthiness convention — was unanticipated.

**H2 outcome (partial).** Mis-routing incidents exist on-tree and are
region-classifiable (§3 failure modes cite them); the navigation guide maps
all four regions; the practitioner-level A/B is designed, not run — reported
as such, never as confirmed.

## 2. Anchored definitions

All eight anchors resolved via `onto_anchor` this session [O1]. Coarse
anchors carry the ruling path per protocol — an operator ruling is requested;
no private definitions are assigned in the meantime. Interim usage follows
the exemplar sources' published senses, which is why §5 exists.

**Update 2026-09-28 (post-rebuild).** The five coarse terms below have since
been ruled into the derived registry (operator ruling 2026-09-28, granted in
the follow-up pass) and resolve live at the derived rung — entries at
`derived.rs:675-718` are the live record. (The composition-time live probes
were recorded in the collaboration report, folded into §4 below and removed
2026-10-01 — git history is the archive.) The table preserves the
composition-time tier with the update noted per row.

| Term | Tier / anchor | Ruling path / published grounding |
|---|---|---|
| `syntax` | coarse at composition → **derived rung** (ruled 2026-09-28, live post-rebuild) | Chomsky (1957) + Tarski (1944) — entry `derived.rs:675`; live probe at composition (report folded 2026-10-01; git history) |
| `semantics` | coarse at composition → **derived rung** (ruled 2026-09-28, live post-rebuild) | Tarski (1944) + Wittgenstein (1953) — entry `derived.rs:683`; live probe at composition (report folded 2026-10-01; git history) |
| `probability` | upper — SUMO `ProbabilityFn` | "The a priori probability of a state of affairs" (Merge.kif, pinned) |
| `determinism` | coarse at composition → **derived rung** (ruled 2026-09-28, live post-rebuild) | Turing (1936) — entry `derived.rs:691`; live probe at composition (report folded 2026-10-01; git history) |
| `computation` | coarse at composition → **derived rung** (ruled 2026-09-28, live post-rebuild) | Turing (1936) + Newell & Simon (1976) — entry `derived.rs:699`; live probe at composition (report folded 2026-10-01; git history) |
| `template` | coarse at composition → **derived rung** (ruled 2026-09-28, live post-rebuild) | Jinja2 (Pallets) + the registry instance — entry `derived.rs:711`; live probe at composition (report folded 2026-10-01; git history) |
| `deterministic_computation` | **derived rung** (operator ruling 2026-09-27) | "Execution with multiplicity 1 … enforces zero conditional entropy relative to the spec." Authority: Turing (1936) |
| `probabilistic_computation` | **derived rung** (operator ruling 2026-09-27) | "A sample from a learned approximation of a posterior … model confidence is not task probability." Authority: Gershman & Goodman (2014); Hu et al. (2024) [S11] |

The two derived-rung rulings are what make the P-axis of this report
non-private: the definitions were ruled with published authority before this
research, and the R-axis terms are flagged where they are not yet ruled.

## 3. The 2×2 model

| | **Deterministic process** (correct by construction) | **Probabilistic process** (correct in expectation) |
|---|---|---|
| **Syntactic representation** (form) | **SD — formal language machinery** | **SP — generation into formal shape** |
| **Semantic representation** (meaning) | **Sem-D — pinned-meaning machinery** | **Sem-P — approximate-meaning machinery** |

The P-axis labels follow [R1]'s restatement: the operative distinction is
correct-by-construction vs correct-in-expectation, not whether the sampler
draws random numbers (temperature-0 objection) — today's micro-test is the
cleanest local instance: run 1 was *correct in expectation* (the spec logic
was right), run 2 *correct by construction* (verified on the machine).

### 3.1 SD — syntactic × deterministic

**Definition.** Membership and transformation in formal languages where the
spec fixes the answer (H(answer|spec) = 0) and a procedure exists.
**Examples.** Grammar parsing, type checking, JSON-schema validation,
ledger arithmetic, decidable language membership (the Dyck pin), regex over
fixed inputs, SQL over structured data. **Tools.** `lisp_eval`, Lean kernel
(`lean_check`), parsers/grammars, schema validators, `cargo`/clippy, git
hooks, the `PortRegistry` typing system. **Failure modes.** Brittleness —
rigid protocols reject valid-but-unexpected input; spec errors remain
upstream epistemic risk (derived-ruling definition [O1]); expert-system
rule explosion [M1]. **Boundary shift.** The protocol-adaptation half of
syntax work (messy input → formal shape) is moving to SP — the deterministic
core stays and gates.

### 3.2 SP — syntactic × probabilistic

**Definition.** Producing formal-shape output (code, schemas, queries,
translations between shapes) from natural or under-specified input, where
correctness is only estimated. **Examples.** Code generation, regex/schema/jq
construction from prose, structured extraction, speech-to-text, protocol
negotiation in natural language. **Tools.** LLM generation with
grammar-constrained decoding / structured output (a deterministic automaton
steering a probabilistic sampler — a region-crossing tool); prompt templates
as deterministic scaffolds (`render_template`: D render, P consumption).
**Failure modes.** Locally well-formed but globally invalid output;
machine-semantic convention mismatch (today's `()`-truthiness defect, §1.4 —
the exhibit); silent partial parses (`extract_json_from_response`
first-`{` bug, on-tree [T1]). **Boundary shift.** Constrained decoding is
actively moving structured extraction from SP toward SD; the gate cost is
the frontier [R1].

### 3.3 Sem-D — semantic × deterministic

**Definition.** Exact computation over meanings where the semantics has been
pinned formally or by registry: published definitions, curated models,
identity resolution. **Examples.** Ontology-anchored term resolution
(`onto_anchor` ladder — published definitions as deterministic semantic
anchors); kernel-checked theorems (meaning-level claims); SQL over curated
models; `resolve_paper` identity resolution; reference-by-ID vs by-name.
**Tools.** `onto_anchor` (derived rung + published sources), Lean proofs,
`resolve_paper`, the research-run ledger, SQLCipher queries.
**Failure modes.** Coverage gaps — only registered senses resolve (five of
eight terms in §2 are coarse today); semantic equivalence between
syntactically distinct expressions is undecidable in general [M1] (decidable
for restricted formalisms — propositional logic, regular languages — the
scope must be stated); curation cost. **Boundary shift.** Operator rulings
grow this region (eight terms graduated in [R1]); embeddings now approximate
the equivalences this region cannot decide — a bridge from Sem-P, not a
replacement.

### 3.4 Sem-P — semantic × probabilistic

**Definition.** Judgment over meanings where no cheap oracle exists:
similarity, relevance, salience, plausibility. **Examples.** Semantic
retrieval, embedding similarity, translation, summarization, hypothesis
generation, calibrated forecasting. **Tools.** `corpus_query` KNN,
`web_search` semantic/deep strategies, curator semantic memory,
superforecasting calibration against resolved outcomes (Brier). **Failure
modes.** Hallucination; semantic drift; the calibration gap (model
confidence is not task probability [O1]); channel fragility — today's direct
evidence: the federated corpus channel was down for this run (§6).
**Boundary shift.** Deterministic gates increasingly check semantic claims at
the boundary (substring verification in grounding-verify; Lean checking
LLM-proposed proofs [S10]); what cannot be gated carries a verification
state instead of a proof — the discipline of this very report.

### 3.5 Crossing patterns (the model's load-bearing structure)

1. **The hybrid loop (propose → verify → revise).** SP/Sem-P generation,
   SD verification, bounded revision. Today's micro-test ran it end-to-end;
   the Lean+LLM line runs it at theorem scale [S10].
2. **The formalization ratchet.** Probabilistic practice → schema/ontology
   anchor → deterministic region. Instances: structured-output APIs;
   derived-registry rulings; [R1]'s compilation ladder. Movement is one-way
   at the artifact level (verified content settles), two-way at the task
   level (rigid protocols get bypassed by natural-language interfaces).
3. **Asymmetric gating (C14).** Syntactic validity is cheap to gate
   (membership in a formal language is decidable — the Dyck pin); semantic
   equivalence is not [M1]. Rule: gate at every syntactic boundary; carry
   verification states across semantic ones. This report's own evidence
   discipline instantiates the rule.

## 4. Navigation guide

**Routing rule (one paragraph).** Locate the task's input and required
output on the R-axis (formal ↔ messy) and its correctness requirement on
the P-axis (guaranteed ↔ expected). Input formal + answer pinned →
deterministic tool, named oracle. Input messy + output formal → LLM proposes,
deterministic gate iterates (the hybrid loop); never ship the proposal
ungated. Output semantic + cheap boundary check exists → gate at the boundary
(substrings, identities, schemas). Output semantic + no cheap oracle →
probabilistic tool with verification states and calibration against resolved
outcomes; emit a calibrated distribution, not a sampled answer, where the
distribution is the deliverable [R1]. Never sample where you can decide;
never trust ungated generation where a gate is cheap.

| Region | Concrete tools/techniques (local instances) |
|---|---|
| SD | `lisp_eval` (battery checks, gate forms); `lean_check` (Dyck pin, axiom audit); schema validators; `cargo`/clippy; git hooks |
| SP | LLM generation + grammar-constrained decoding/structured output; `render_template` as deterministic scaffold for probabilistic output; revision loops driven by gate feedback |
| Sem-D | `onto_anchor` (ladder + derived rung); Lean theorems; `resolve_paper`; `begin_research_run` ledger; SQL over curated models |
| Sem-P | `corpus_query` KNN retrieval; `web_search` (semantic/deep); curator semantic memory; superforecasting + Brier calibration; verification states for ungated semantic claims |

Region-crossing tools to know: constrained decoding (SP machinery
enforcing SD invariants); embeddings (Sem-P judgment on Sem-D-stored
content); the research ledger (Sem-D record of Sem-P search). Known strain
(flagged under grill-me edge cases): embeddings are deterministic code
computing over probabilistic semantics — assign the region by what the tool
*guarantees* (nothing, Sem-P) while noting the machine is D; the model
survives this only by that convention, stated here rather than hidden.

### The three-party collaboration overlay (folded 2026-10-01)

The collaboration report (`navigating-the-region-space-collaboration.md`,
removed 2026-10-01 per the operator's P1 ruling; git history is the
archive) modeled the parties that navigate this space. Its load-bearing
claim [K13, inferred]: **each party is the preferred oracle for a
different part of the space, and the collaboration breaks when a party
is used where another's oracle is cheaper.**

| Party | SD (form × deterministic) | SP (form × probabilistic) | Sem-D (meaning × deterministic) | Sem-P (meaning × probabilistic) |
|---|---|---|---|---|
| **Operator (human)** | ratifies oracle authority; supplies the spec determinism enforces | confirms *intent* at messy-input boundaries | grants rulings; ground-truths scored outcomes | **the oracle**: spec authority, acceptance, what the work is FOR |
| **Curator (regulator)** | maintains the D/P legend, thresholds, calibration records | records skill-use failures against the generating skill | owns the derived registry, evidence-cited memory, rulings record | consults memory; escalates domain concerns |
| **Z-K agent (executor)** | runs the oracles (`lisp_eval`, `lean_check`, `cargo`) | generates into formal shape; iterates against gates | resolves anchors; keeps ledgers and citations verbatim | carries verification states; drafts; asks — never assumes consent |

Three consequences (each a testable OUGHT): the operator is load-bearing
exactly where machines are weakest — Sem-P scope questions, which is why
the goal loop vests scoring and acceptance in the operator alone; the
Curator's distinctive function is the formalization ratchet's operator
(converting episodic probabilistic experience into deterministic
structure — no other party both remembers across sessions and holds the
registry); the agent's distinctive function is the traverse — the only
party that routinely crosses all four regions in one task, obligated to
gate at every syntactic handoff and carry verification states across
semantic spans. The report's machinery classification found every
on-tree collaboration tool is a **shared representation** — a grounding
medium in Clark's sense (§5) — sitting at a region boundary rather than
inside a region. The reified machinery (the `region-routing` skill) and
its validation record live in the skill
(`.agents/skills/region-routing/SKILL.md`) and the skills registry.

## 5. Exemplars and their checkable sources

All sources recorded in research run `b6f6206e00fae965` (server-side ledger,
`recorded_by='server'`): 7 web searches (21 results) + 5 arXiv resolutions
[R3]. No attribution below is memory-only; none is fabricated. Where a
primary document was reachable it is cited as primary.

| Exemplar | Usable idea for this space | Source |
|---|---|---|
| Tarski (1944) | Semantic conception of truth: semantics made exact *for formalized languages* — the Sem-D dream and its scope limit | [S1] primary PDF |
| Chomsky (1957) | Syntax as an autonomous formal system — the SD extraction of form from behavior | [S2] |
| Wittgenstein (1953) | Meaning as use / language games — semantics lives in practice; the boundary H1 leans on (LLMs learn use-statistics) | [S3] |
| Pearl (1988) | Bayesian networks — principled Sem-P; and deterministic code performing probabilistic inference ([R1]'s independence cell) | [S4] |
| McCarthy (1959) | Advice Taker: formal-language premises + heuristic advice — the propose-layer-over-deduction pattern; heuristic vs algorithmic | [S5] primary PDF |
| Newell & Simon (1976) | Physical symbol system hypothesis — the syntax-suffices historical pole the 2×2 measures against | [S6] primary PDF |
| Goodman et al. (2008, Church) | Probabilistic programming: a probabilistic guest language evaluated on a deterministic host — H1's layering in PL form | [S7] |
| Marcus (2020) | Neurosymbolic four steps — the hybrid-loop argument against pure deep learning | [S8] arXiv:2004.09933 |
| Lake et al. (2016) | Compositionality + causal models + learning-to-learn as the systematicity gap | [S9] arXiv:1604.00289 |
| LeanDojo (2023) | LLM proposes tactics, Lean kernel verifies — the SP→SD gate at theorem scale | [S10] arXiv:2306.15616 |
| Hu, Jain & Elmoznino (2024) | LLMs as amortized inference — H1's mechanism, published | [S11] arXiv:2310.04363 |
| Floridi (2025) | Certainty/scope distinction for LLM reliability — region-level certainty typing | [S12] arXiv:2506.10130 |

*Not pursued this run (candidates from the mission, not dropped silently):*
Frege and Montague — Tarski covers the form-vs-meaning line for this
report's purposes; both remain open for a follow-up pass.

## 6. Capability log

| Capability | Resolution state |
|---|---|
| `hypothesis-framer` | Resolved; loaded; applied — FINER table (§1.2), PICO (§1.3), two-cycle convergence gate (cycle 1 honestly `false`, cycle 2 `true`) |
| `lean-prover` | Resolved; loaded; applied — Dyck pin checked: exit 0, Lean 4.34.0, `dyck_prefix_dip` axiom-free; on-tree negative control `kask/lean/negative_control.lean` (a checker accepting `1 = 2` is not an oracle) |
| `falsifiability` | Resolved; loaded; applied — Popper admissibility (H1 IS-core admitted; H2 split), elimination verdict `multiple_corroborated` via D form |
| `grill-me` | Resolved; loaded; applied — 5-question ladder over the model; round 1: 3 Solid / 2 Partial → gate `hold`; round 2 (Rice's-theorem scoping; calibrated-distribution branch added to the routing rule) → gate `complete`; both gates D via `lisp_eval` |
| `metacognition` | Resolved; loaded; applied — prior calibrations read (`kanban_goal_list`); prediction goal `7b0deed5` created pre-test; outcome fail; Brier 0.16 (D) pending operator resolution |
| `pragmatic-semantics` | Resolved; loaded; applied — D/P labels across the report (header note, §3, §4); region claims routed to their verification regime |
| Research MCP server | Resolved — run `b6f6206e00fae965`; 26 source records server-side [R3] |
| `onto_anchor` | Resolved; applied — 8 anchors (§2) |
| John Brooks replica / knowledge base | **Partial — degraded, surfaced.** All three `curator_federated_search` queries failed: the embedding model (`openrouter/qwen/qwen3-embedding-8b`) cannot use the embedding port bound to DeepInfra — collateral of the in-flight migration goal `74cfe8a2` (continue). No false skill attribution filed (no owning skill in this cascade). Fallback used: session-loaded corpus passages [M1], labeled unverified-ID, plus [R1]'s verified corpus record IDs secondhand. Retry queued as an open question. **Update 2026-09-28 (post-rebuild):** the embedding port is fixed and curator-memory federated search works; the corpus source remains unavailable — the v13 calibration run directory is absent (re-embedding migration collateral). C12 stays partial with this updated reason |
| `capabilities-reasoner` (deleted 2026-09-24) | **Resolution confirmed:** its role — capability limits: what a designed probe elicits vs default observation — is covered by `metacognition` + `pragmatic-semantics` (per [R1]), with `skill-discovery` as routing layer. **Instantiated today:** the battery probe elicited a defect invisible to default observation. No remaining gap routed; `skill-discovery` not needed this run |
| Curator status | Algedonic log 200/200 (approaching cap), 13 review cards not done — noted per protocol, not acted on; log-maintenance is an operator decision |

## 7. Verification-state table

States from the six-value lattice; every claim's citation resolves in
**Sources**. Reconciliation check in §9.

| Claim | Statement (compressed) | State | Cites |
|---|---|---|---|
| C1 | 2×2 is the user-confirmed base model; graded refinement is open-question extension | verified | U1 |
| C2 | P-axis terms resolve on the derived rung (Turing 1936; Gershman & Goodman 2014; Hu et al. 2024) | verified | O1 |
| C3 | Five R-axis/P-axis terms resolved only coarse (5W1H) at composition; rulings requested, none assigned privately. **Update 2026-09-28:** all five ruled and live at the derived rung (probes recorded at composition — report folded 2026-10-01, git history) | verified (composition-time; superseded by the landed ruling) | O1 |
| C4 | `probability` resolves to SUMO `ProbabilityFn` | verified | O1 |
| C5 | First-shot artifact failed 5/12 vectors: 4 accepted-invalid + 1 type-error crash | verified | E1 |
| C6 | One gate-driven revision closed it: 12/12 pass | verified | E1 |
| C7 | Root cause is a machine-semantic encoding mismatch (monocausal; differential pattern + targeted fix confirm) | inferred | E1 |
| C8 | Dyck membership decidability pinned in Lean, `dyck_prefix_dip` axiom-free (4.34.0, exit 0) | verified | E2 |
| C9 | On-tree historical instances of the ungated-defect class (`extract_json` first-`{`; reversed `string-contains` args) | partial | T1 |
| C10 | H1's "expose semantics" half: mechanism grounded, cost comparison unmeasured | inferred | S11 |
| C11 | 12 exemplar attributions carry ledger-recorded checkable sources | verified | R3 |
| C12 | Brooks-corpus claims (equivalence undecidability; expert-system brittleness; statistical semantics) | partial | M1 |
| C13 | Mis-routing incidents exist on-tree and are region-classifiable (H2 IS-core) | partial | T1 |
| C14 | Asymmetric gating principle: gate syntax cheaply, carry verification states for semantics | inferred | E1, R1 |
| C15 | Correct-by-construction vs correct-in-expectation restatement resolves the temperature-0 objection | verified | R1 |

## 8. Open questions

1. **Graded refinement of both axes (the optional extension).** Read R as a
   continuum (fully formal → schematic → natural) and P as one (exact →
   bounded-error → statistical → judgment); the 2×2 becomes a lattice and
   "region" becomes a neighborhood. The micro-test suggests the most useful
   single gradation: *distance to the nearest cheap gate*.
2. **Further axes.** Tractability ([R1] Q3, the chess cell), world-closure
   ([R1] §3.2), verification-cost frontier, and knowledge-source (written-down
   function vs exhibited-in-data, the Gershman/Hu split).
3. **Does any signal on the R-axis predict SP defects?** Today's defect was
   invisible in the artifact's syntax and logic; only the machine revealed
   it. If no generator-side signal tracks machine-semantic convention
   mismatch, every SP artifact needs an external gate — the routing
   question [R1] Q1 generalized to the joint space.
4. **H1(c) and the rate claim.** Run the deterministic-only arm and an N>1
   battery family; measure the ungated defect rate across artifact classes.
5. **H2 practitioner test.** Present the guide; measure routing changes
   against the incident census.
6. **Corpus re-query** after migration goal `74cfe8a2` lands — the blocked
   channel's retry, to move C12 from partial toward verified.
7. **Ontology rulings — RESOLVED 2026-09-28:** the five terms are ruled
   into the derived registry (`derived.rs:675-718`) and resolve live at the
   derived rung post-rebuild; §2's table carries the per-row update.

## 9. Check ledger

All checks below were executed via `lisp_eval` (D) over the report's
extracted tables *before saving*; results embedded as run.

| Check | Result |
|---|---|
| All 4 regions mapped to ≥1 tool | Run, passed (counts 2/2/2/2) |
| Exemplar count ≥5 with ledger sources | Run, passed (12) |
| Claim/state/citation count reconciliation (15 = 15 = 15) | Run, passed |
| All states from the six-value lattice | Run, passed |
| All cited keys resolve in Sources | Run, passed |
| FINER convergence gate (cycle 1 / cycle 2) | Run: `false` (honest, weak dims recorded) / `true` |
| Grill-me escalation gates (round 1 / round 2) | Run: `[5, hold]` / `[5, complete]` |
| Falsifiability elimination verdict | Run: `multiple_corroborated` |
| Metacognition Brier | Run: 0.16 (operator resolution pending) |
| Lean axiom audit (`dyck_prefix_dip`) | Run: axiom-free, exit 0, Lean 4.34.0 |
| First-shot battery / revision battery | Run: 5/12 defective / 12/12 pass |
| Not run (reported, not claimed): H1(c) deterministic-only arm; H2 practitioner A/B; direct corpus re-query (channel blocked) | — |

## Sources

- **[U1]** User mission statement for this report (2×2 confirmation; hypotheses; pipeline spec), 2026-09-28.
- **[O1]** `onto_anchor` session runs, 2026-09-28 — 6 term anchors + 2 derived-rung anchors (rulings of 2026-09-27 recorded in `hkask-bridge-ontology/src/derived.rs`, per [R1] §Ontology anchors).
- **[E1]** H1 micro-test, this session — first-shot and revision batteries via `lisp_eval` (runs 1–2, §1.4).
- **[E2]** `lean_check` on `kask/lean/dyck_decidable.lean`, this session.
- **[T1]** On-tree traps: `.rules` — `extract_json_from_response` first-`{` parsing; `string-contains` reversed args (2026-09-28); `unwrap_or(0)` feedback-loop rule.
- **[R1]** `deterministic-vs-probabilistic-compute-routing.md` (on-tree, v1.0.1, 2026-09-28) — P-axis, entropy, routing, closure; corpus record IDs `5b6efe78`, `b4947fb5`, `49f36d8c` cited secondhand.
- **[R2]** `artificial-curiosity-capability-space.md` (on-tree, 2026-09-28) — probe discipline, capability-space verdicts.
- **[R3]** Research run `b6f6206e00fae965` ledger (begin_research_run; web_search ×7; resolve_paper ×5), 2026-09-28.
- **[M1]** Session-loaded John Brooks corpus passages (curator memory, 2026-09-28): "it is difficult, if not impossible… to unequivocally decide the semantic equivalence of two syntactically distinct entities"; expert-systems brittleness ("many rules to avoid absurd conclusions… less robust"); Firth's-axiom statistical semantics ("more robust than" formal). Unverified-ID: direct corpus re-query blocked (§6) — honest limit, not silently used as verified.
- **[S1]** Tarski, "The Semantic Conception of Truth", *Philosophy and Phenomenological Research* 4(3), 1944 — https://sites.ualberta.ca/~francisp/Phil426/TarskiTruth1944.pdf (primary).
- **[S2]** Chomsky, *Syntactic Structures*, 1957 — https://en.wikipedia.org/wiki/Syntactic_Structures; https://philopedia.org/works/syntactic-structures/.
- **[S3]** Wittgenstein, *Philosophical Investigations*, 1953 — https://en.wikipedia.org/wiki/Language_game_(philosophy); https://aeon.co/ideas/how-playing-wittgensteinian-language-games-can-set-us-free.
- **[S4]** Pearl, *Probabilistic Reasoning in Intelligent Systems*, Morgan Kaufmann, 1988 — https://dl.acm.org/doi/book/10.5555/534975; https://www.sciencedirect.com/book/monograph/9780080514895.
- **[S5]** McCarthy, "Programs with Common Sense", 1959 — https://www-formal.stanford.edu/jmc/mcc59.pdf (primary).
- **[S6]** Newell & Simon, "Computer Science as Empirical Inquiry: Symbols and Search", *CACM* 19(3), 1976 — https://iiif.library.cmu.edu/file/Newell_box00024_fld01660_doc0003/Newell_box00024_fld01660_doc0003.pdf (primary).
- **[S7]** Goodman, Mansinghka, Roy, Bonawitz & Tenenbaum, "Church: a language for generative models", UAI 2008 — https://web.stanford.edu/~ngoodman/papers/POPL2013-abstract.pdf (citing entry); https://dl.acm.org/doi/abs/10.1145/3591290.
- **[S8]** Marcus, "The Next Decade in AI: Four Steps Towards Robust Artificial Intelligence", 2020 — https://arxiv.org/abs/2004.09933.
- **[S9]** Lake, Ullman, Tenenbaum & Gershman, "Building Machines That Learn and Think Like People", 2016 — https://arxiv.org/abs/1604.00289.
- **[S10]** Yang, Wang, Lauto, et al., "LeanDojo: Theorem Proving with Retrieval-Augmented Language Models", 2023 — https://arxiv.org/abs/2306.15616.
- **[S11]** Hu, Jain & Elmoznino, "Amortizing Intractable Inference in Large Language Models", 2024 — https://arxiv.org/abs/2310.04363.
- **[S12]** Floridi et al., 2025 — https://arxiv.org/abs/2506.10130.

*Report composed under the mission's phased pipeline (frame → ground → prior
art → model → formal pinning → stress-test → compose); research ledger
`b6f6206e00fae965`; goal `ee73663f`.*