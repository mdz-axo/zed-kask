---
name: hypothesis-framer
description: "Research question framing and hypothesis formulation using FINER criteria and PICO process. Evaluates topics through Feasibility, Interest, Novelty, Ethics, and Relevance gates, structures questions, derives testable hypotheses."
audience: user
---

# Hypothesis Framer

Research question framing and hypothesis formulation using FINER criteria and PICO process. Evaluates broad research topics through Feasibility, Interest, Novelty, Ethics, and Relevance (FINER) gates, structures questions via Population-Intervention-Comparison-Outcome (PICO) framework, derives testable hypotheses with null hypothesis formulation, operationalizes into aims and objectives, and verifies alignment. Iterative PDCA refinement until the question-hypothesis-aims chain is coherent and testable.

## Reference models

FINER — Hulley, Cummings et al., *Designing Clinical Research* (1988; 4th ed. 2013). PICO — Richardson, Wilson, Nishikawa & Hayward, "The well-built clinical question", *ACP Journal Club* 123 (1995). Testability delegates to `falsifiability` (Popper). `onto_anchor` → derived `finer_criteria` and `pico` (operator ruling 2026-09-25).

## Conditions and labels

- **Initial condition (T1):** the broad topic, domain and research context, measured by step 1's per-dimension FINER scores; on a refinement cycle, the prior cycle's gate inputs (misalignments, weak dimensions, testability, admissibility, feasibility).
- **Target condition (T2):** the step 13 gate passes.
- **D/P labelling:** steps 1–12 are P (judgment), critiqued by the `falsifiability-admit` delegation (step 10), the five-link alignment check (step 11), and the operator. Step 13 is D (`lisp_eval` `HF-3`, one form over the three renders' objects — it also derives `overall_finer_rating` for step 2 and the objectives-count/δ-margin checks of step 10). FINER scores are ordinal judgments; do not average them or treat a 7 as a measured quantity — the gate uses only the dimensions scoring below 7.

## When to Use

- When you have a broad research topic that needs evaluation against FINER criteria (Feasibility, Interest, Novelty, Ethics, Relevance) before committing to a study design
- When a research question needs structuring via the PICO framework (Population, Intervention, Comparison, Outcome) to ensure precision and testability
- When you need to derive a testable hypothesis with null hypothesis formulation from a PICO-structured question
- When hypothesis operationalization into research aims and objectives is required, with alignment verification and feasibility recheck
- When iterative PDCA refinement is needed to converge the question-hypothesis-aims chain into a coherent, testable, decision-ready framing — including the convergence gate (FINER compliance, PICO completeness, hypothesis coherence, aims alignment) that terminates the loop

## When NOT to Use

- Bug root-cause hypotheses — `diagnose` + `falsifiability` own that shape; PICO is clinical/research framing.
- Testability adjudication alone — `falsifiability`'s admissibility gate is the shared instrument; this skill builds the full question-hypothesis-aims chain.
- Forecasting questions — use `superforecasting`; a forecast is not a research hypothesis.

## Instructions

1. **Evaluate the broad research topic against FINER criteria.** For each of the five dimensions — Feasible (subjects, expertise, resources, institutional support), Interesting (audience, applicability, engagement), Novel (knowledge gap, methodology, confirmation), Ethical (regulatory compliance, risk, informed consent, animal welfare), and Relevant (clinical impact, knowledge contribution, generalizability, timeliness) — assign a score from 0–10 with specific, justified rationale.
2. **Identify the most concerning FINER dimension** (lowest score) and read `overall_finer_rating` from the step-13 form `HF-3` run over the `finer-evaluate` render (`critical_flaw` below 3, `weak` below 7, `strong` at 8 or above, otherwise `adequate`; `unscored` when any dimension lacks a numeric score) — the same form, same scores, run early so the refinement sees the rating. Formulate a refined research question that addresses the weaknesses. Provide actionable refinement suggestions for each dimension scoring below 7. The refined question must be a question (not a declarative statement), hypothesis-driven (not data-driven), and open inquiry rather than closeable with yes/no.
3. **Apply the PICO framework** to structure the refined research question. Define the Population (condition, demographics, setting, inclusion/exclusion criteria, justification), Intervention (type, description, dose/intensity/frequency, duration, delivery), Comparison (type, description, justification — acknowledge if no comparator exists), and Outcome (primary and secondary outcomes, measurement methods, timing, clinical significance).
4. **Synthesize the PICO elements into a single structured question** using the appropriate template (intervention, diagnostic, prognostic, or etiology format). Assess PICO completeness for each element as complete, partial, or missing. Assemble `pico_elements` — the object bundling pico-structure's four outputs (`population`, `intervention`, `comparison`, `outcome`) — for the next step's render.
5. **Determine the hypothesis type** — difference, association, superiority, non-inferiority, equivalence, diagnostic accuracy, or prognostic — based on the PICO structure and study design.
6. **Formulate the research hypothesis (H₁)** as a declarative statement predicting the expected outcome. Reference PICO elements explicitly, use directional language when possible, and ensure falsifiability. Format: "In [population], [intervention] will [direction] [outcome] compared to [comparison]."
7. **Formulate the null hypothesis (H₀)** for the chosen design: no difference/no relationship for superiority, difference or association; inferior by at least δ for non-inferiority; outside ±δ for equivalence. Do not use a no-difference null for margin-based designs.
8. **Define the primary aim** as a broad, overarching purpose directly linked to the research question, referencing PICO elements. Format: "The primary aim of this study is to [verb] [what] in [population]."
9. **Define 2–4 primary objectives** as specific, measurable steps that accomplish the primary aim, linked to the primary outcome measure. Define secondary aims with clear rationale if applicable — avoid "fishing expeditions."
10. **Assess testability** now that objectives are specified: verify measurable outcome with validated method, specified population, defined comparison, suggested statistical test, clinically meaningful effect size, non-inferiority/equivalence margin (δ) if applicable, and sample size feasibility. The primary objectives count `[2..4]` and CONSORT δ-margin presence are checked by `HF-3` over the `hypothesis-operationalize` render (`objectives_ok` from `(length primary_objectives)`; `margin_ok` requires a non-blank `testability_assessment.non_inferiority_margin` when `hypothesis_type` is `non_inferiority` or `equivalence`). Render `falsifiability/falsifiability-admit` with the formulated H₁ as `target`, the domain, and PICO/objectives as `context`; consume its admissibility result. Rendering is not a test result: if no concrete falsifying observation exists, mark the framing not testable.
11. **Verify five-link alignment**: question→hypothesis, hypothesis→primary aim, primary aim→objectives, objectives→PICO outcome, and hypothesis→null hypothesis. Flag any misalignments honestly and propose corrections.
12. **Recheck feasibility** in light of operational aims and objectives: sample size, methods, timeline, and resources. Identify any new concerns that emerged during aims/objectives development.
13. **Check convergence (the gate `HF-3`).** Call `lisp_eval` with:
    - form: `(begin (define obj (lambda (v) (if (listp v) v (list)))) (define get (lambda (k o) (if (listp o) (assoc k o) nil))) (define text (lambda (v) (if (stringp v) v ""))) (define nonblank (lambda (v) (and (stringp v) (> (length v) 0)))) (define dims (list "feasible" "interesting" "novel" "ethical" "relevant")) (define score-of (lambda (d) (get "score" (get d (obj finer_scores))))) (define weak (lambda (ds) (if (is_null ds) (list) (let ((s (score-of (car ds)))) (if (and (numberp s) (>= s 7)) (weak (cdr ds)) (cons (car ds) (weak (cdr ds)))))))) (define min-score (lambda (ds m) (if (is_null ds) m (let ((s (score-of (car ds)))) (min-score (cdr ds) (if (numberp s) (if (or (is_null m) (< s m)) s m) m)))))) (define all-scored (lambda (ds) (if (is_null ds) true (and (numberp (score-of (car ds))) (all-scored (cdr ds)))))) (define weak_finer (weak dims)) (define m (min-score dims nil)) (define rating (cond ((not (all-scored dims)) "unscored") ((< m 3) "critical_flaw") ((< m 7) "weak") ((>= m 8) "strong") (t "adequate"))) (define op (obj operationalize)) (define ta (obj (get "testability_assessment" op))) (define hyp-type (text (get "hypothesis_type" op))) (define margin-needed (or (string= hyp-type "non_inferiority") (string= hyp-type "equivalence"))) (define margin-ok (if margin-needed (nonblank (get "non_inferiority_margin" ta)) true)) (define testable (and (string= (text (get "overall_testability" ta)) "testable") margin-ok)) (define objectives (length (obj (get "primary_objectives" op)))) (define objectives-ok (and (>= objectives 2) (<= objectives 4))) (define misalignments (length (obj (get "misalignments" (obj (get "alignment_check" op)))))) (define feasible (string= (text (get "overall_feasibility" (obj (get "feasibility_recheck" op)))) "confirmed")) (define admitted (eq admissible true)) (list (list "converged" (and (= misalignments 0) (is_null weak_finer) objectives-ok testable admitted feasible)) (list "weak_finer" weak_finer) (list "overall_finer_rating" rating) (list "misalignment_count" misalignments) (list "objectives_count" objectives) (list "objectives_ok" objectives-ok) (list "hypothesis_type" hyp-type) (list "margin_ok" margin-ok) (list "testable" testable) (list "admissible" admitted) (list "feasible" feasible)))`
    - env: `{ "finer_scores": <the finer-evaluate render's finer_scores object — five dimensions, each {score, rationale}>, "operationalize": <the hypothesis-operationalize render, whole — the form reads hypothesis_type, testability_assessment, primary_objectives, alignment_check and feasibility_recheck>, "admissible": <falsifiability-admit's admissible boolean> }`
    - returns `{converged, weak_finer, overall_finer_rating, misalignment_count, objectives_count, objectives_ok, hypothesis_type, margin_ok, testable, admissible, feasible}` — `converged` requires zero misalignments, no dimension below 7, 2–4 primary objectives, `overall_testability` `testable` (with a δ margin for margin-based designs), `admissible` exactly `true`, and `overall_feasibility` `confirmed`. Pass the renders' objects as returned — never a retyped list of weak dimensions or a hand-judged `testable`; a missing or null object fails the fields it feeds (a null `admissible` is not admitted; a missing score is `unscored` and weak).
    Bound: max 2 refinement cycles — on a failing gate, re-enter step 2
    (refine the question) with the flagged weaknesses; misalignments that
    survive the second cycle are reported honestly (step 11's
    flag-don't-paper-over rule). A remaining failed testability or admissibility
    gate is reported as `not_testable`/`blocked`, never `testable-with-reservations`.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `finer-evaluate.j2` | Apply FINER criteria to a broad research topic. Evaluate feasibility (subjects, expertise, resources), interest (audience relevance), novelty (knowledge contribution), ethics (compliance), and relevance (clinical impact). Produces per-dimension scores with rationale and refinement suggestions. |
| `pico-structure.j2` | Apply PICO framework to structure the research question. Identifies Population characteristics, Intervention/exposure, Comparison/control, and Outcome measures. Produces a structured question and completeness assessment. |
| `hypothesis-operationalize.j2` | Single cognitive act: derive a testable hypothesis from the PICO-structured question, formulate the null hypothesis, classify type, operationalize into research aims and objectives, assess testability against the specified objectives, verify five-link alignment, and recheck feasibility. Merges what were previously separate hypothesis and aims steps — because you cannot assess testability without knowing the objectives. |

To render a template, call the `render_template` tool with the template ref (e.g., `hypothesis-framer/finer-evaluate`) and a context object with the required variables.
Template context variables (from each template's [inference] contract):
- `finer-evaluate.j2`: `broad_topic`, `domain`, `research_context`, `question_hint`
- `hypothesis-operationalize.j2`: `structured_question`, `pico_elements`, `domain`, `finer_result`
- `pico-structure.j2`: `research_question`, `domain`, `finer_result`, `population_hints`
## Regression case

`HF-3` receipts over the renders' real shapes (`finer_scores.{feasible,interesting,novel,ethical,relevant}.score`, the whole `hypothesis-operationalize` object, `admissible`), executed 2026-11-13 through the `hkask_lisp` evaluator and re-evaluated by `~/Documents/zk-data/skills/hypothesis-framer/2026-11-13-hf-repair/receipts.sh` (fixture: all scores 8–9, superiority, two objectives, no misalignments, feasibility confirmed, admissible true → `converged` true, rating `strong`). Each row changes one thing:

| change | converged | field that names it |
|---|---|---|
| `ethical.score` 6 | false | `weak_finer` `["ethical"]`, rating `weak` |
| `feasible.score` 2 / 3 / 7 | — | rating `critical_flaw` / `weak` / `adequate` (7 converges) |
| `novel` has no score | false | rating `unscored`, `weak_finer` `["novel"]` |
| one `alignment_check.misalignments` entry | false | `misalignment_count` 1 |
| `overall_testability` `partially_testable` | false | `testable` false |
| `hypothesis_type` `non_inferiority`, margin null | false | `margin_ok` false, `testable` false |
| `non_inferiority` with margin `"0.1 (absolute)"` | true | `margin_ok` true |
| `equivalence` with margin `""` | false | `margin_ok` false — blank is absent |
| `admissible` false / null | false | `admissible` false |
| `overall_feasibility` `new_concerns` | false | `feasible` false |
| 1 or 5 primary objectives | false | `objectives_ok` false (4 converges) |
| all three inputs null | false | every field at its failing value |

Before this repair the gate took `weak_finer`, `testable`, `misalignment_count` and `feasible` as model-typed values read off the same renders (HF-1 and HF-2 took their inputs the same way); the receipts could only show that typed booleans conjoin.

## Constraints

- The research question must be a question, not a declarative statement; the research hypothesis must be a declarative statement, not a question
- The null must match the design: no difference/no relationship for difference or association; a justified δ-boundary null for non-inferiority or equivalence
- Non-inferiority and equivalence hypotheses require a defined δ margin — without it, the hypothesis is not testable
- Aims must be broader than objectives; objectives must be measurable — "to improve understanding" is not measurable
- Secondary aims must have clear rationale — avoid "nice to know" add-ons
- The alignment check must be honest — flag misalignments, do not paper over them
- If PICO comparison is "none," acknowledge the descriptive/pre-post design limitation rather than fabricating a comparator
