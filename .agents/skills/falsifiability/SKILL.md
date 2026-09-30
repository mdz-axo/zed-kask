---
name: falsifiability
description: "Domain-agnostic eliminative inference engine. Rules out untestable questions (admissibility gate), generates multiple falsifiable hypotheses, constructs minimal counterfactuals, designs discriminating tests, and eliminates hypotheses that fail."
---

# Falsifiability

Domain-agnostic eliminative inference engine anchored to Popper (falsifiability), Platt (strong inference), and Chamberlin (multiple working hypotheses), with Pearl/Halpern counterfactual reasoning as the alternative generator. Rules out what is not testable at the question level (admissibility gate), generates multiple falsifiable hypotheses, constructs minimal counterfactuals, designs discriminating tests, and eliminates the hypotheses that fail — corroborating the survivors, never confirming them. A delegation target: diagnose, hypothesis-framer, and superforecasting delegate their falsification stages here; metacognition's inquiry experiment delegates here when a counterfactual scenario must be explored.

## Reference models

Popper, *The Logic of Scientific Discovery* (1959); Platt, "Strong Inference", *Science* 146 (1964); Chamberlin, "The Method of Multiple Working Hypotheses", *Science* 15 (1890); Pearl, *Causality* (2009) and Halpern & Pearl (2005) for the do-operator. `onto_anchor` → derived `falsifiability` (operator ruling 2026-09-25).

**D/P labelling.** Steps 1–4 are P: admission, hypotheses, counterfactuals and tests are judgment, critiqued by the user's review (steps 2 and 4 present for it) and by the observations themselves. Step 5's per-hypothesis eliminate/corroborate call is P (does this observation contradict this prediction?), recorded in the auditable `falsification_log`. Step 5's verdict, step 6's materiality guard, and step 6's convergence composite (the 0.50 verdict / 0.30 alternatives-eliminated / 0.20 remainder weights, computed over the cumulative eliminated set) are D (`lisp_eval`, forms below) over the counts step 5 recorded.

## When to Use

- When you need to decide whether a claim or question is *testable at all* before committing resources to investigating it — the Popper admissibility gate.
- When a single explanation has been adopted and you need to force consideration of alternatives — Chamberlin's multiple working hypotheses prevent premature anchoring.
- When a causal claim ("X causes Y") needs stress-testing via its counterfactual — "if X had not occurred, would Y still obtain?" — the Pearl/Halpern do-operator.
- When you need discriminating tests that rule out hypotheses, not tests that merely confirm a favorite — Platt's strong inference.
- When hypotheses must be eliminated by failed predictions (hard falsification), not down-weighted by evidence (that is superforecasting's Bayesian concern).
- When `diagnose` generates falsifiable hypotheses and needs the shared elimination method rather than its bug-specific reimplementation.
- When `hypothesis-framer` assesses testability and needs the shared admissibility gate rather than its PICO-specific reimplementation.
- When `superforecasting` stage_3 (inside view) generates necessary conditions and evidence for/against each causal hypothesis and needs the shared counterfactual + elimination engine.
- When `metacognition`'s inquiry experiment branches on "a counterfactual scenario must be explored" and needs a delegation target (`metacognition/inquiry-delegate-falsifiability`).
- When evaluating whether an elimination cycle has converged — one corroborated survivor with all alternatives ruled out — or has plateaued with an irreducible remainder.

## When NOT to Use

- Bayesian belief updating — evidence that down-weights rather than eliminates is `superforecasting`'s concern; this skill kills hypotheses, it does not re-rate them.
- Formal proof of program properties — use `lean-prover`; machine-checked proof is stronger than falsification.
- Generating research questions — use `hypothesis-framer`; this skill adjudicates and eliminates what already exists.

## Instructions

1. **Admit the target (Popper gate).** Before generating any hypotheses, test whether the claim or question under analysis is testable at all. Classify it on the pragmatic-semantics axes (IS/OUGHT, declarative/probabilistic/subjunctive, constraint force). State the concrete observation that, if witnessed, would contradict it. ADMIT only if a genuine falsifying observation exists and the target is an IS-mode claim (or a subjunctive claim whose counterfactual is testable). RULE OUT tautologies, pure OUGHTs, and unfalsifiable-by-construction claims, recording the reason. If the target is ruled out but salvageable, propose a refined testable reformulation — then RE-GATE the refined form through `falsifiability-admit` (the refined target needs its own falsifying observation; the original's describes the ruled-out claim) and pass the re-admitted form as `admitted_target` to hypothesize. Do not proceed to hypothesizing with an inadmissible target.

2. **Generate multiple working hypotheses (Chamberlin + Platt).** For the admitted target, generate 3–7 candidate explanations with no early commitment. Force diversity: include at least one unlikely hypothesis, at least one that challenges the obvious explanation, and at least one embarrassing-if-true. Each carried hypothesis must state a prediction ("if X is the case, then observation Y will obtain under condition Z") and a falsifier (the concrete observation that would prove it wrong). Discard any candidate that cannot be made falsifiable — record it as discarded with a reason, do not carry it forward. Rank by likelihood, not ease of testing. Present the ranked list for user review before proceeding; the user's domain knowledge re-ranks instantly.

3. **Construct counterfactuals (Pearl/Halpern).** For each surviving hypothesis, identify the proposed cause (X) and the claimed effect (Y). Construct the minimal counterfactual: "in a world identical to ours except that X did not occur (do(not X)), would Y still obtain?" Apply the do-operator surgically — remove only X, hold confounders and background fixed, do not manipulate anything downstream of X (that is what you are testing) or anything upstream that merely correlates with X (that is a confounder to hold fixed). Derive the testable consequence: the observable difference between the factual and counterfactual worlds. If a clean intervention is infeasible, name a natural experiment or proxy — or say `none` honestly. Flag hypotheses whose cause cannot be intervened on (ethical, physical, or structural) as irreducible; they survive by default but are marked not-counterfactually-testable, which limits how much they can be corroborated.

4. **Design discriminating tests (Platt).** Design tests whose outcome rules out at least one hypothesis. The cardinal error is one-test-per-hypothesis — a test that can only confirm your favorite is a comfort blanket, not a discriminating test. A test is discriminating only if at least two hypotheses predict different outcomes for it. Prefer tests that falsify multiple hypotheses in one observation (maximize elimination power). Build a coverage matrix mapping each test × hypothesis to `falsifies` / `corroborates` / `neutral`. Every hypothesis must be falsifiable by at least one designed test; if not, add a test or flag the hypothesis untestable-by-available-means. Flag hypothesis pairs that predict identical outcomes for every testable design as irreducible — they survive together and the user must be told the evidence cannot choose between them. Rank tests by elimination power, not ease of running. Present for user review.

5. **Eliminate and corroborate.** Apply the observations. A hypothesis whose falsifiable prediction is contradicted is eliminated — hard, not probabilistic. Record each elimination with the test, the prediction, the observation, and the contradiction (auditable falsification log). A hypothesis that predicted the observed outcome is corroborated — it withstood a test that could have falsified it. Corroborated is not confirmed: surviving does not make a hypothesis more likely in any absolute sense, only more resilient. A falsifiable hypothesis this cycle's observations were NEUTRAL toward — neither matched nor contradicted — is neither corroborated nor eliminated; count it with the untested. Hypotheses flagged irreducible or not falsifiable by any available test survive by default — record them as survived_by_default with the reason; the user must understand these were not tested, only that nothing could test them. Choose the verdict in this order: with no observations this cycle, `nothing_eliminated` (corroborated stays empty); if all hypotheses were eliminated, `none_corroborated`; if any survivor lacks a corroborating observation this cycle — survived-by-default or neutral — `untested_remainder` (including one corroborated plus one untested, even when no hypothesis was eliminated); if none was eliminated this cycle, `nothing_eliminated`; if exactly one was corroborated and all alternatives eliminated, `one_corroborated_survivor`; otherwise, with two or more corroborated survivors, `multiple_corroborated`. Report default survivors even in a no-observation cycle; never promote an untested survivor to corroborated. Compute the verdict with `lisp_eval` (the two `nothing_eliminated` branches are distinct cases — no observations, versus observations that eliminated nothing — and their order is load-bearing):
   - form: `(cond ((= observations 0) "nothing_eliminated") ((= eliminated total) "none_corroborated") ((> untested 0) "untested_remainder") ((= eliminated 0) "nothing_eliminated") ((and (= corroborated 1) (= eliminated (- total 1))) "one_corroborated_survivor") (t "multiple_corroborated"))`
   - env (all five counts are THIS CYCLE's, and they are DISJOINT — total = eliminated + corroborated + untested; each carried hypothesis is exactly one of the three this cycle): `{ "observations": <observations applied this cycle>, "total": <hypotheses carried into the cycle>, "eliminated": <eliminated this cycle>, "corroborated": <corroborated this cycle>, "untested": <survivors without a corroborating observation this cycle — survived_by_default plus neutral-toward-this-cycle> }`. Convergence (step 6) consumes the CUMULATIVE eliminated set (the `prior_eliminated` input to `falsifiability-eliminate` plus this cycle's `eliminated` output) for its alternatives-eliminated proportion; the verdict itself is this-cycle.

6. **Check convergence.** Measure whether the elimination has pared the hypothesis space to one corroborated survivor with all alternatives eliminated (convergence 0) or nothing has been ruled out (the practical maximum, 0.95 under the ordinal mapping — 1.0 is a theoretical bound the mapping never reaches, because a no-observation cycle still scores `nothing_eliminated` at 0.1). The verdict dimension carries weight 0.50; the alternatives-eliminated proportion 0.30; the reducible remainder 0.20. Compute the composite with `lisp_eval` — two forms:
   - verdict-score form: `(cond ((string= verdict "one_corroborated_survivor") 1.0) ((string= verdict "multiple_corroborated") 0.5) ((string= verdict "untested_remainder") 0.3) ((string= verdict "nothing_eliminated") 0.1) (t 0.0))`
   - env: `{ "verdict": <the step-5 verdict string> }` — the ordinal mapping (operator ruling 2026-09-29: one_corroborated_survivor=1.0, multiple_corroborated=0.5, untested_remainder=0.3, nothing_eliminated=0.1, none_corroborated=0.0)
   - composite form: `(+ (* 0.5 (- 1 verdict_score)) (* 0.3 (- 1 eliminated_proportion)) (* 0.2 reducible_remainder))`
   - env: `{ "verdict_score": <from the verdict-score form>, "eliminated_proportion": <cumulative eliminated / (total − 1) — the proportion of ALTERNATIVES eliminated; at full convergence (one corroborated survivor of N, N−1 eliminated) it is 1.0>, "reducible_remainder": <untested survivors / total — the OPEN-WORK proportion; corroborated survivors and irreducible flags are not open work, so at full convergence it is 0.0> }` — convergence 0 = fully converged (verdict_score 1.0, all alternatives eliminated, nothing untested); the practical maximum is 0.95 (a no-observation cycle: verdict_score 0.1, nothing eliminated, all untested). The `eliminated_proportion` uses the CUMULATIVE eliminated set (the `prior_eliminated` input to `falsifiability-eliminate` plus this cycle's `eliminated` output); `reducible_remainder` is the open-work proportion (more untested = more work = higher convergence).
   Apply the materiality guard with `lisp_eval` `(and (= eliminated 0) (not new_test_available) (< (abs (- metric metric_prior)) 0.02))` — `eliminated` is THIS CYCLE's count (not the cumulative set — the guard asks whether this cycle made progress); `new_test_available` is a judgment (P) supplied to the form, not computed by it; `metric`/`metric_prior` are the convergence composite this cycle vs the prior cycle's (the same composite form's output, two consecutive runs). If it holds, force convergence — the residual gap is irreducible, not a fixable defect, and the honest answer is the bounded remainder, not infinite iteration. Blockers: `untested_remainder` cannot converge to a unique survivor; report the untested hypotheses and the missing discriminating test. A no-observation `nothing_eliminated` verdict also carries the untested set and cannot claim a unique survivor. `none_corroborated` is a hard block (restart, do not iterate); `nothing_eliminated` with no new test available is a stall; `multiple_corroborated` with no new discriminating test is an irreducible remainder to report, not iterate past.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `falsifiability-admit.j2` | Popper admissibility gate. For the claim or question under analysis, test whether a concrete observation could contradict it. Rule out unfalsifiable targets (tautologies, unfalsifiable OUGHTs, protected claims) with a recorded reason; admit only what survives. Classifies testability via the pragmatic-semantics IS/OUGHT and epistemic-mode axes. |
| `falsifiability-hypothesize.j2` | Chamberlin multiple working hypotheses. Generate 3-7 candidate explanations for the admitted question with no early commitment. Each hypothesis must carry a falsifiable prediction in Platt form — if X, then observation Y under condition Z. Hypotheses with no possible falsifying observation are discarded at generation, not carried forward. |
| `falsifiability-counterfactual.j2` | Pearl/Halpern counterfactual generation. For each surviving hypothesis, construct the minimal counterfactual — if the proposed cause had not occurred (do(not X)), would the effect still obtain? — and derive the concrete testable consequence that distinguishes the counterfactual world from the factual one. |
| `falsifiability-discriminate.j2` | Platt discriminating-test design. Design tests whose outcome rules out at least one hypothesis — not one test per hypothesis. Map each test to the hypotheses it can falsify. Flag hypothesis pairs no available test can discriminate as irreducible and surface them for the user. |
| `falsifiability-eliminate.j2` | Apply evidence and eliminate. Hypotheses whose falsifiable predictions fail are ruled out — hard elimination, not Bayesian down-weighting (that is superforecasting's concern). Survivors are corroborated, never confirmed. Record which test eliminated which hypothesis and the falsifying observation. |

To render a template, call the `render_template` tool with the template ref (e.g., `falsifiability/falsifiability-admit`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `falsifiability-admit.j2`: `target`, `domain`, `context`
- `falsifiability-hypothesize.j2`: `admitted_target`, `domain`, `context`
- `falsifiability-counterfactual.j2`: `hypotheses`, `admitted_target`, `domain`
- `falsifiability-discriminate.j2`: `hypotheses`, `counterfactuals`, `domain`
- `falsifiability-eliminate.j2`: `hypotheses`, `discriminating_tests`, `observations` (a map of test description → observed outcome — the results of the tests run this cycle), `prior_eliminated`, `irreducible` (from the counterfactual render), `irreducible_pairs` (from the discriminate render)

## Regression case

Run the verdict form through `lisp_eval` across its six branches: no
observations → `nothing_eliminated`; all eliminated → `none_corroborated`;
any survivor without a corroborating observation this cycle (survived-by-
default or neutral) → `untested_remainder` — the load-bearing precedence:
for one corroborated plus one untested, the clause order blocks
`nothing_eliminated` and `multiple_corroborated` (the clause-4 and
default branches land after clause 3), while `one_corroborated_survivor`
is blocked arithmetically (it requires `eliminated = total − 1`, which
forces `untested = 0` under the disjoint accounting); a no-observation
cycle WITH default survivors also reports `nothing_eliminated` (branch 1
precedes branch 3 — the second load-bearing cross-case); observations but
zero eliminated → `nothing_eliminated`; exactly one corroborated with all
alternatives eliminated → `one_corroborated_survivor`; two or more
corroborated → `multiple_corroborated`. Run the materiality guard both
ways: all conditions met with no new test available and a metric delta
strictly under 0.02 → true (force convergence — the remainder is
irreducible); the same env with `new_test_available` true → false. The
boundary is strict `<`: a delta of exactly 0.02 → false. All branch
receipts executed through the live tool 2026-09-29.

## Composition

This skill is designed as a **delegation target**, mirroring the architectural
role of `mcda` and `diagnose`:

- **metacognition** (inquiry experiment) has `falsifiability` as one of its
  four delegation targets, for counterfactual branches.
- **diagnose** step 3 (generate 3–7 falsifiable root-cause hypotheses)
  delegates to `falsifiability-hypothesize` (its own admission gate stays —
  a reproduced bug is already admitted). Diagnose's steps 5–7 (probe,
  fix, convergence) apply the elimination method through their own probe
  design; only the hypothesize seam is wired today.
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

Refactoring the four consumers above to delegate here (rather than each
reimplementing the method) follows the strangler-fig pattern: one domain at
a time, system functional at every step. Three further skills already
delegate here — `gradient-hunter` (gradient-hypothesize), `eqm` (evidence
grounding for confirmation_bias), and `verification-compression`
(experiment design) — their seams are recorded in their own SKILL.mds.
`code-review` is an eighth consumer by pattern-embedding rather than
delegation: its adjudicate phase carries the H0/H1/falsifier framing inline
(a finding with no falsifier is a preference, not a defect).

## Constraints

- Corroborated is not confirmed. Never output "proven", "verified true", or "established." Use "survived", "withstood", "corroborated."
- Elimination is hard, not probabilistic. A contradicted prediction rules the hypothesis out; do not down-weight and carry it (that is superforecasting's job).
- A hypothesis with no possible falsifying observation is inadmissible at generation, not "weak" — it leaves the pool, recorded.
- A discriminating test must be able to rule out at least one hypothesis. A test that only confirms the favorite is not discriminating.
- The do-operator must be surgical: remove only the proposed cause, hold confounders fixed. "If things were different" is not a counterfactual.
- An irreducible hypothesis is flagged, not eliminated — it survives by default but is marked not-counterfactually-testable, which limits corroboration.
- If every hypothesis is eliminated, the verdict is `none_corroborated` — the framing is wrong and must be restarted from hypothesize, not iterated.
