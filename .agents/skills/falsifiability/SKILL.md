---
name: falsifiability
description: "Domain-agnostic eliminative inference engine. Rules out untestable questions (admissibility gate), generates multiple falsifiable hypotheses, constructs minimal counterfactuals, designs discriminating tests, and eliminates hypotheses that fail."
---

# Falsifiability

Domain-agnostic eliminative inference engine anchored to Popper (falsifiability), Platt (strong inference), and Chamberlin (multiple working hypotheses), with Pearl/Halpern counterfactual reasoning as the alternative generator. Rules out what is not testable at the question level (admissibility gate), generates multiple falsifiable hypotheses, constructs minimal counterfactuals, designs discriminating tests, and eliminates the hypotheses that fail — corroborating the survivors, never confirming them. A delegation target: diagnose, hypothesis-framer, and superforecasting delegate their falsification stages here; sequential-inquiry may delegate when a counterfactual scenario must be explored.

## When to Use

- When you need to decide whether a claim or question is *testable at all* before committing resources to investigating it — the Popper admissibility gate.
- When a single explanation has been adopted and you need to force consideration of alternatives — Chamberlin's multiple working hypotheses prevent premature anchoring.
- When a causal claim ("X causes Y") needs stress-testing via its counterfactual — "if X had not occurred, would Y still obtain?" — the Pearl/Halpern do-operator.
- When you need discriminating tests that rule out hypotheses, not tests that merely confirm a favorite — Platt's strong inference.
- When hypotheses must be eliminated by failed predictions (hard falsification), not down-weighted by evidence (that is superforecasting's Bayesian concern).
- When `diagnose` generates falsifiable hypotheses and needs the shared elimination method rather than its bug-specific reimplementation.
- When `hypothesis-framer` assesses testability and needs the shared admissibility gate rather than its PICO-specific reimplementation.
- When `superforecasting` stage_3 (inside view) generates necessary conditions and evidence for/against each causal hypothesis and needs the shared counterfactual + elimination engine.
- When `sequential-inquiry` branches on "a counterfactual scenario must be explored" and needs a delegation target — currently a dead reference this skill wires up.
- When evaluating whether an elimination cycle has converged — one corroborated survivor with all alternatives ruled out — or has plateaued with an irreducible remainder.

## Instructions

1. **Admit the target (Popper gate).** Before generating any hypotheses, test whether the claim or question under analysis is testable at all. Classify it on the pragmatic-semantics axes (IS/OUGHT, declarative/probabilistic/subjunctive, constraint force). State the concrete observation that, if witnessed, would contradict it. ADMIT only if a genuine falsifying observation exists and the target is an IS-mode claim (or a subjunctive claim whose counterfactual is testable). RULE OUT tautologies, pure OUGHTs, and unfalsifiable-by-construction claims, recording the reason. If the target is ruled out but salvageable, propose a refined testable reformulation. Do not proceed to hypothesizing with an inadmissible target.

2. **Generate multiple working hypotheses (Chamberlin + Platt).** For the admitted target, generate 3–7 candidate explanations with no early commitment. Force diversity: include at least one unlikely hypothesis, at least one that challenges the obvious explanation, and at least one embarrassing-if-true. Each carried hypothesis must state a prediction ("if X is the case, then observation Y will obtain under condition Z") and a falsifier (the concrete observation that would prove it wrong). Discard any candidate that cannot be made falsifiable — record it as discarded with a reason, do not carry it forward. Rank by likelihood, not ease of testing. Present the ranked list for user review before proceeding; the user's domain knowledge re-ranks instantly.

3. **Construct counterfactuals (Pearl/Halpern).** For each surviving hypothesis, identify the proposed cause (X) and the claimed effect (Y). Construct the minimal counterfactual: "in a world identical to ours except that X did not occur (do(not X)), would Y still obtain?" Apply the do-operator surgically — remove only X, hold confounders and background fixed, do not manipulate anything downstream of X (that is what you are testing) or anything upstream that merely correlates with X (that is a confounder to hold fixed). Derive the testable consequence: the observable difference between the factual and counterfactual worlds. If a clean intervention is infeasible, name a natural experiment or proxy — or say `none` honestly. Flag hypotheses whose cause cannot be intervened on (ethical, physical, or structural) as irreducible; they survive by default but are marked not-counterfactually-testable, which limits how much they can be corroborated.

4. **Design discriminating tests (Platt).** Design tests whose outcome rules out at least one hypothesis. The cardinal error is one-test-per-hypothesis — a test that can only confirm your favorite is a comfort blanket, not a discriminating test. A test is discriminating only if at least two hypotheses predict different outcomes for it. Prefer tests that falsify multiple hypotheses in one observation (maximize elimination power). Build a coverage matrix mapping each test × hypothesis to `falsifies` / `corroborates` / `neutral`. Every hypothesis must be falsifiable by at least one designed test; if not, add a test or flag the hypothesis untestable-by-available-means. Flag hypothesis pairs that predict identical outcomes for every testable design as irreducible — they survive together and the user must be told the evidence cannot choose between them. Rank tests by elimination power, not ease of running. Present for user review.

5. **Eliminate and corroborate.** Apply the observations. A hypothesis whose falsifiable prediction is contradicted is eliminated — hard, not probabilistic. Record each elimination with the test, the prediction, the observation, and the contradiction (auditable falsification log). A hypothesis that predicted the observed outcome is corroborated — it withstood a test that could have falsified it. Corroborated is not confirmed: surviving does not make a hypothesis more likely in any absolute sense, only more resilient. Hypotheses flagged irreducible or not falsifiable by any available test survive by default — record them as survived_by_default with the reason; the user must understand these were not tested, only that nothing could test them. Compute the verdict: `one_corroborated_survivor` (strong-inference ideal), `multiple_corroborated` (tests ran but did not fully discriminate), `none_corroborated` (framing wrong — restart from hypothesize, not iterate), or `nothing_eliminated` (no test ruled anything out).

6. **Check convergence.** Measure whether the elimination has pared the hypothesis space to one corroborated survivor with all alternatives eliminated (convergence 0) or nothing has been ruled out (convergence 1). The verdict dimension carries weight 0.50; the alternatives-eliminated proportion 0.30; the irreducible remainder 0.20. Apply the materiality guard: if no hypothesis was eliminated this cycle, no new discriminating test is available, and the metric delta vs the prior cycle is < 0.02, force convergence — the residual gap is irreducible, not a fixable defect, and the honest answer is the bounded remainder, not infinite iteration. Blockers: `none_corroborated` is a hard block (restart, do not iterate); `nothing_eliminated` with no new test available is a stall; `multiple_corroborated` with no new discriminating test is an irreducible remainder to report, not iterate past.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `falsifiability-admit.j2` | KnowAct | Popper admissibility gate. For the claim or question under analysis, test whether a concrete observation could contradict it. Rule out unfalsifiable targets (tautologies, unfalsifiable OUGHTs, protected claims) with a recorded reason; admit only what survives. Classifies testability via the pragmatic-semantics IS/OUGHT and epistemic-mode axes. |
| `falsifiability-hypothesize.j2` | KnowAct | Chamberlin multiple working hypotheses. Generate 3-7 candidate explanations for the admitted question with no early commitment. Each hypothesis must carry a falsifiable prediction in Platt form — if X, then observation Y under condition Z. Hypotheses with no possible falsifying observation are discarded at generation, not carried forward. |
| `falsifiability-counterfactual.j2` | KnowAct | Pearl/Halpern counterfactual generation. For each surviving hypothesis, construct the minimal counterfactual — if the proposed cause had not occurred (do(not X)), would the effect still obtain? — and derive the concrete testable consequence that distinguishes the counterfactual world from the factual one. |
| `falsifiability-discriminate.j2` | KnowAct | Platt discriminating-test design. Design tests whose outcome rules out at least one hypothesis — not one test per hypothesis. Map each test to the hypotheses it can falsify. Flag hypothesis pairs no available test can discriminate as irreducible and surface them for the user. |
| `falsifiability-eliminate.j2` | KnowAct | Apply evidence and eliminate. Hypotheses whose falsifiable predictions fail are ruled out — hard elimination, not Bayesian down-weighting (that is superforecasting's concern). Survivors are corroborated, never confirmed. Record which test eliminated which hypothesis and the falsifying observation. |

## Composition

This skill is designed as a **delegation target**, mirroring the architectural
role of `mcda` and `diagnose`:

- **sequential-inquiry** gains `falsifiability` as a fourth delegation target
  (its engine already branches when "a counterfactual scenario must be
  explored" — currently a dead reference this skill wires up).
- **diagnose** step 3 (generate 3–5 falsifiable hypotheses) and its elimination
  logic delegate to `falsifiability-hypothesize` + `falsifiability-discriminate`
+ `falsifiability-eliminate`, keeping its bug-specific codegraph ontological anchoring (Dublin Core + PKO).
- **hypothesis-framer** step 10 (testability assessment) delegates to
  `falsifiability-admit`.
- **superforecasting** inside view is split: hypothesis generation delegates
  to `falsifiability-hypothesize` (Chamberlin/Platt) and necessary-conditions
  counterfactual analysis to `falsifiability-counterfactual` (Pearl do-operator);
  probability estimation and the Bayesian update (`stage_4_evidence_update`)
  stay in superforecasting. The clean seam: this skill *eliminates* by hard
  falsification; superforecasting *reweights* by Bayesian updating. They are
  complementary, not competing — a hypothesis can be eliminated here (ruled
  out) and down-weighted there (made unlikely); the former is terminal, the
  latter revisable.

Refactoring the three consumers to delegate here (rather than each
reimplementing the method) follows the strangler-fig pattern: one domain at a
time, system functional at every step.

## Constraints

- rJoule cap: 2 per invocation. Maximum 10 iterations.
- `falsifiability-admit.j2`: Public.
- `falsifiability-hypothesize.j2`: Public.
- `falsifiability-counterfactual.j2`: Public.
- `falsifiability-discriminate.j2`: Public.
- `falsifiability-eliminate.j2`: Public.
- Corroborated is not confirmed. Never output "proven", "verified true", or "established." Use "survived", "withstood", "corroborated."
- Elimination is hard, not probabilistic. A contradicted prediction rules the hypothesis out; do not down-weight and carry it (that is superforecasting's job).
- A hypothesis with no possible falsifying observation is inadmissible at generation, not "weak" — it leaves the pool, recorded.
- A discriminating test must be able to rule out at least one hypothesis. A test that only confirms the favorite is not discriminating.
- The do-operator must be surgical: remove only the proposed cause, hold confounders fixed. "If things were different" is not a counterfactual.
- An irreducible hypothesis is flagged, not eliminated — it survives by default but is marked not-counterfactually-testable, which limits corroboration.
- If every hypothesis is eliminated, the verdict is `none_corroborated` — the framing is wrong and must be restarted from hypothesize, not iterated.
- Do not execute arbitrary Python code in Jinja2 expressions (sandboxed execution).
- Handle missing variables gracefully (leave as-is or use default if specified).
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.