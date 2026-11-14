---
name: skill-discovery
description: "Match tasks to installed skills and acquire NEW skills when none fits. Route a task or gap against the catalog with fit scores, detect and classify capability gaps, and evaluate candidate skills against format/quality/safety criteria before installation."
audience: platform
---

# Skill Discovery

Match tasks to the installed skill catalog and acquire NEW skills when nothing fits. One fit-scoring instrument serves both questions: **route** ranks installed skills for a task or a capability gap and emits uncovered capabilities; **detect-gap** classifies those uncovered capabilities; **evaluate** vets a candidate before installation.

## When to Use

- Match a task or work slice against the installed skill catalog to find the best-fitting skill(s) (route).
- Consume `skill_match_query` fields emitted by `task-breakdown`'s decompose phase, `metacognition`'s `skill_match_queries`, or `kata-improvement`'s step-8 output (one per slice) and return ranked recommendations (route).
- Detect capability gaps in the registry corpus by comparing task patterns against existing skills (detect-gap).
- Evaluate a candidate skill against format, quality, and safety criteria to decide whether it should be installed, revised, or rejected (evaluate).

## When NOT to Use

- Authoring a skill from scratch — `create-skill`; discovery evaluates candidates, it does not write them.
- Auditing or maintaining installed skills — `skill-maintenance`.
- Executing the matched skill — invoke it directly; a recommendation is not a dispatch.

## Reference model and D/P labelling

The fit score is a **heuristic ranking** in the form of a fixed-weight
weighted-sum value model (Belton & Stewart, *Multiple Criteria Decision
Analysis*, 2002 — the same model `mcda` cites; anchored: `onto_anchor` →
`multi_criteria_decision_analysis`, tier `domain_supplement`
(`cso:multi-criteria_decision_analysis`, CSO 3.5; derived entry Belton &
Stewart 2002, operator ruling 2026-09-25; verified 2026-11-02)), applied
WITHOUT the sensitivity analysis that model prescribes: nothing here tests
whether the 0.50 / 0.25 / 0.25 split changes any ranking. The weights (inherited
from skill-router before the 2026-09-24 fold), the +0.20 epistemic boost
and the 24-of-32 installable threshold (rulings 2026-09-26), and the 0.30
floor and the 0.8 / 0.4 coverage bands (the band boundary fixed 2026-09-25)
are operator-set constants, not values derived from measured routing
outcomes. Treat a fit score as an ordering aid whose arithmetic is
reproducible, not as a calibrated probability that the skill fits; the
constants are open to revision when routing outcomes are recorded against
them.

The three fit dimensions (capability overlap, description alignment, trigger
alignment), gap classification, candidate evaluation, the `epistemic_state`
self-report (P — unmeasured by any tool, disclosed as such), the
boost-applicability judgment (which skills count as certainty-finding),
detect-gap's impact scoring, priority ranking, and per-gap recommended
action are P — judgment, critiqued by the recomputation below and by the
caller's epistemic state. The composite, the +0.20 boost arithmetic, the
0.30 floor and the coverage band are D: recompute them from the route
output with the `lisp_eval` form in skill-discovery-route item 4 before
acting on it — a recommendation whose arithmetic was not recomputed is
unverified routing.

## Instructions

### skill-discovery-route

1. For each candidate skill in the catalog, compute a fit_score in [0.0, 1.0] across three dimensions: capability overlap (0.50), description alignment (0.25), trigger alignment (0.25).
2. Composite fit_score = (capability × 0.50) + (description × 0.25) + (trigger × 0.25).
3. Rank recommendations by fit_score descending; return at most `max_recommendations` (default 3), only those with fit_score ≥ 0.30.
4. Classify coverage: `full` (≥1 skill at fit ≥ 0.80), `partial` (best fit ≥ 0.40 and < 0.80), `none` (best fit < 0.40).
   **D over P:** the three dimension scores are judgments (P); the composite, the +0.20 boost, the 0.30 floor and the band are arithmetic (D). Recompute them over the whole route output in one `lisp_eval` call (`SD-0`) before acting on it:
   - form: `(begin (define text-of (lambda (row key) (if (stringp (assoc key row)) (assoc key row) ""))) (define num (lambda (obj key) (let ((v (assoc key obj))) (if (and (numberp v) (>= v 0) (<= v 1)) v nil)))) (define recs (if (listp recommendations) recommendations (list))) (define boost (if (and (listp boost_applied) (eq (assoc "applied" boost_applied) t)) 0.2 0)) (define boosted (if (and (listp boost_applied) (listp (assoc "skills" boost_applied))) (assoc "skills" boost_applied) (list))) (define fit-of (lambda (r) (let ((d (assoc "dimension_scores" r))) (if (not (listp d)) nil (let ((c (num d "capability_overlap")) (de (num d "description_alignment")) (tr (num d "trigger_alignment"))) (if (or (is_null c) (is_null de) (is_null tr)) nil (let ((b (if (member (text-of r "skill_name") boosted) boost 0))) (min 1 (max 0 (+ (* 0.5 c) (* 0.25 de) (* 0.25 (min 1 (+ tr b))))))))))))) (define rows (lambda (rs) (if (is_null rs) (list) (cons (list (list "skill_name" (text-of (car rs) "skill_name")) (list "fit" (fit-of (car rs))) (list "eligible" (and (numberp (fit-of (car rs))) (>= (fit-of (car rs)) 0.3)))) (rows (cdr rs)))))) (define table (rows recs)) (define malformed (lambda (ts) (if (is_null ts) (list) (if (numberp (assoc "fit" (car ts))) (malformed (cdr ts)) (cons (text-of (car ts) "skill_name") (malformed (cdr ts))))))) (define best-of (lambda (ts acc) (if (is_null ts) acc (best-of (cdr ts) (if (and (numberp (assoc "fit" (car ts))) (> (assoc "fit" (car ts)) acc)) (assoc "fit" (car ts)) acc))))) (define best (best-of table 0)) (define eligible (lambda (ts) (if (is_null ts) 0 (+ (if (eq (assoc "eligible" (car ts)) t) 1 0) (eligible (cdr ts)))))) (define cap (if (numberp max_recommendations) max_recommendations 3)) (list (list "verdict" (cond ((not (is_null (malformed table))) "malformed_scores") ((> (length recs) cap) "too_many_recommendations") (t "ok"))) (list "fits" table) (list "best_fit" best) (list "coverage" (cond ((is_null recs) "none") ((>= best 0.8) "full") ((>= best 0.4) "partial") (t "none"))) (list "eligible_count" (eligible table)) (list "boost" boost) (list "malformed" (malformed table))))`
   - env: `{ "recommendations": <the route render's recommendations array, with each dimension_scores object>, "boost_applied": <the route render's boost_applied object — its skills list names the skills the boost applied to>, "max_recommendations": <the cap, default 3> }`
   - receipts (beside the form; `check-skill-lisp-forms` evaluates each line's env over the form above and compares the expected result exactly):
     ```lisp-receipts
     {"gate": "SD-0", "case": "boosted-trigger-clamps-at-1", "env": {"recommendations": [{"rank":1,"skill_name":"falsifiability","dimension_scores":{"capability_overlap":0.9,"description_alignment":0.8,"trigger_alignment":0.9}}], "boost_applied": {"applied": true, "reason": "confidence 0.3", "skills": ["falsifiability"]}, "max_recommendations": 3}, "expected": {"verdict":"ok","fits":[{"skill_name":"falsifiability","fit":0.9,"eligible":true}],"best_fit":0.9,"coverage":"full","eligible_count":1,"boost":0.2,"malformed":[]}}
     {"gate": "SD-0", "case": "mid-band-is-partial", "env": {"recommendations": [{"rank":1,"skill_name":"a","dimension_scores":{"capability_overlap":0.5,"description_alignment":0.5,"trigger_alignment":0.5}}], "boost_applied": {"applied": false, "reason": "no epistemic_state supplied"}, "max_recommendations": 3}, "expected": {"verdict":"ok","fits":[{"skill_name":"a","fit":0.5,"eligible":true}],"best_fit":0.5,"coverage":"partial","eligible_count":1,"boost":0,"malformed":[]}}
     {"gate": "SD-0", "case": "0.79-is-partial", "env": {"recommendations": [{"rank":1,"skill_name":"a","dimension_scores":{"capability_overlap":0.79,"description_alignment":0.79,"trigger_alignment":0.79}}], "boost_applied": {"applied": false, "reason": "no epistemic_state supplied"}, "max_recommendations": 3}, "expected": {"verdict":"ok","fits":[{"skill_name":"a","fit":0.79,"eligible":true}],"best_fit":0.79,"coverage":"partial","eligible_count":1,"boost":0,"malformed":[]}}
     {"gate": "SD-0", "case": "0.80-is-full", "env": {"recommendations": [{"rank":1,"skill_name":"a","dimension_scores":{"capability_overlap":0.8,"description_alignment":0.8,"trigger_alignment":0.8}}], "boost_applied": {"applied": false, "reason": "no epistemic_state supplied"}, "max_recommendations": 3}, "expected": {"verdict":"ok","fits":[{"skill_name":"a","fit":0.8,"eligible":true}],"best_fit":0.8,"coverage":"full","eligible_count":1,"boost":0,"malformed":[]}}
     {"gate": "SD-0", "case": "0.39-is-none", "env": {"recommendations": [{"rank":1,"skill_name":"a","dimension_scores":{"capability_overlap":0.39,"description_alignment":0.39,"trigger_alignment":0.39}}], "boost_applied": {"applied": false, "reason": "no epistemic_state supplied"}, "max_recommendations": 3}, "expected": {"verdict":"ok","fits":[{"skill_name":"a","fit":0.39,"eligible":true}],"best_fit":0.39,"coverage":"none","eligible_count":1,"boost":0,"malformed":[]}}
     {"gate": "SD-0", "case": "weighted-weak-fit-below-floor", "env": {"recommendations": [{"rank":1,"skill_name":"a","dimension_scores":{"capability_overlap":0.3,"description_alignment":0.2,"trigger_alignment":0.1}}], "boost_applied": {"applied": false, "reason": "no epistemic_state supplied"}, "max_recommendations": 3}, "expected": {"verdict":"ok","fits":[{"skill_name":"a","fit":0.225,"eligible":false}],"best_fit":0.225,"coverage":"none","eligible_count":0,"boost":0,"malformed":[]}}
     {"gate": "SD-0", "case": "best-fit-drives-the-band", "env": {"recommendations": [{"rank":1,"skill_name":"a","dimension_scores":{"capability_overlap":0.3,"description_alignment":0.2,"trigger_alignment":0.1}},{"rank":1,"skill_name":"b","dimension_scores":{"capability_overlap":0.9,"description_alignment":0.9,"trigger_alignment":0.9}}], "boost_applied": {"applied": false, "reason": "no epistemic_state supplied"}, "max_recommendations": 3}, "expected": {"verdict":"ok","fits":[{"skill_name":"a","fit":0.225,"eligible":false},{"skill_name":"b","fit":0.9,"eligible":true}],"best_fit":0.9,"coverage":"full","eligible_count":1,"boost":0,"malformed":[]}}
     {"gate": "SD-0", "case": "empty-recommendations-is-none", "env": {"recommendations": [], "boost_applied": {"applied": false, "reason": "no epistemic_state supplied"}, "max_recommendations": 3}, "expected": {"verdict":"ok","fits":[],"best_fit":0,"coverage":"none","eligible_count":0,"boost":0,"malformed":[]}}
     {"gate": "SD-0", "case": "out-of-range-score-is-malformed-and-named", "env": {"recommendations": [{"rank":1,"skill_name":"a","dimension_scores":{"capability_overlap":1.2,"description_alignment":0.2,"trigger_alignment":0.1}}], "boost_applied": {"applied": false, "reason": "no epistemic_state supplied"}, "max_recommendations": 3}, "expected": {"verdict":"malformed_scores","fits":[{"skill_name":"a","fit":null,"eligible":false}],"best_fit":0,"coverage":"none","eligible_count":0,"boost":0,"malformed":["a"]}}
     {"gate": "SD-0", "case": "over-cap-is-too-many", "env": {"recommendations": [{"rank":1,"skill_name":"a","dimension_scores":{"capability_overlap":0.5,"description_alignment":0.5,"trigger_alignment":0.5}},{"rank":1,"skill_name":"a","dimension_scores":{"capability_overlap":0.5,"description_alignment":0.5,"trigger_alignment":0.5}},{"rank":1,"skill_name":"a","dimension_scores":{"capability_overlap":0.5,"description_alignment":0.5,"trigger_alignment":0.5}},{"rank":1,"skill_name":"a","dimension_scores":{"capability_overlap":0.5,"description_alignment":0.5,"trigger_alignment":0.5}}], "boost_applied": {"applied": false, "reason": "no epistemic_state supplied"}, "max_recommendations": 3}, "expected": {"verdict":"too_many_recommendations","fits":[{"skill_name":"a","fit":0.5,"eligible":true},{"skill_name":"a","fit":0.5,"eligible":true},{"skill_name":"a","fit":0.5,"eligible":true},{"skill_name":"a","fit":0.5,"eligible":true}],"best_fit":0.5,"coverage":"partial","eligible_count":4,"boost":0,"malformed":[]}}
     {"gate": "SD-0", "case": "boost-lifts-only-the-named-skill", "env": {"recommendations": [{"rank":1,"skill_name":"a","dimension_scores":{"capability_overlap":0.5,"description_alignment":0.5,"trigger_alignment":0.5}},{"rank":1,"skill_name":"b","dimension_scores":{"capability_overlap":0.5,"description_alignment":0.5,"trigger_alignment":0.5}}], "boost_applied": {"applied": true, "skills": ["a"]}, "max_recommendations": 3}, "expected": {"verdict":"ok","fits":[{"skill_name":"a","fit":0.55,"eligible":true},{"skill_name":"b","fit":0.5,"eligible":true}],"best_fit":0.55,"coverage":"partial","eligible_count":2,"boost":0.2,"malformed":[]}}
     {"gate": "SD-0", "case": "boost-not-applied-lifts-nothing", "env": {"recommendations": [{"rank":1,"skill_name":"a","dimension_scores":{"capability_overlap":0.5,"description_alignment":0.5,"trigger_alignment":0.5}}], "boost_applied": {"applied": false, "skills": ["a"]}, "max_recommendations": 3}, "expected": {"verdict":"ok","fits":[{"skill_name":"a","fit":0.5,"eligible":true}],"best_fit":0.5,"coverage":"partial","eligible_count":1,"boost":0,"malformed":[]}}
     ```
   - returns `fits` (per skill: the recomputed `fit` and whether it clears the 0.30 floor), `best_fit`, `coverage` (`full` ≥ 0.80, `partial` ≥ 0.40, else `none`; `none` for an empty list), `eligible_count`, and `verdict` `ok` | `malformed_scores` (a dimension missing or outside 0–1 — named) | `too_many_recommendations`. The boost is applied only to the skills `boost_applied.skills` names, and only when `applied` is true.
5. If coverage is partial or none, emit `uncovered_capabilities` (the detect-gap input): each with `capability`, `task_pattern`, `closest_skill`, `gap_type` (coverage|feature|epistemic).
6. `epistemic_state` is optional and self-reported by the caller; no skill or tool measures it today. When it is provided with confidence < 0.5, apply a +0.20 boost to trigger-alignment for certainty-finding skills; clamp to [0.0, 1.0]. Record `boost_applied` (true/false) and its reason in the output so the boost can be checked.
7. Do not recommend skill-discovery as a match — it is a meta-skill.
8. Respond with a JSON object from the route template (`recommendations`, `uncovered_capabilities`, `boost_applied`) plus the `lisp_eval`-computed `fit_score` and `coverage_assessment`, and verify route consistency via `lisp_eval` (`SD-1`): `(and (<= recs_count max_recs) (if (string= coverage "full") (= uncovered_count 0) true) (if (string= coverage "none") (> uncovered_count 0) true))`.
9. Re-entry: when coverage is partial or none AND the catalog has changed since this route ran (a candidate was installed), re-run route once against the grown catalog. Bound: max 2 routing passes per task; a second partial result emits the gap signals and stops.

### skill-discovery-detect-gap

1. Map every task pattern to determine if an existing skill covers it fully, partially, or not at all.
2. Classify partial matches as Feature gaps rather than Coverage gaps.
3. Detect latent gaps where a quality or governance rule exists in `kask/docs/architecture/core/PRINCIPLES.md` but no skill enforces it, classifying these as Governance gaps.
4. Score the impact of each gap on agent effectiveness as `critical`, `high`, `medium`, or `low`.
5. Prioritize gaps by impact, then by frequency of the associated task pattern.
6. Recommend an action per gap: `create_skill` (coverage gap), `extend_skill` (feature gap, needs new templates), `route_to_existing_skill` (feature gap, existing skill not yet routed — confirm with route), `discover_external` (external crate), `automate` (automation gap), or `ignore` (low-impact only).
7. Respond with a JSON object containing the `gap_list` and `priority_ranking`.
8. Evaluate every task pattern without skipping niche patterns.
9. Ensure each gap references exactly one category, even if it spans multiple task patterns.
10. Justify impact scoring using task pattern frequency and consequence.
11. Do not recommend `ignore` for any gap with `critical` or `high` impact. Verify completeness and non-ignore invariants in `lisp_eval` (`SD-2`): `(and (= patterns_evaluated total_patterns) (= high_critical_ignored_count 0))`.
12. Classify `epistemic` gaps distinctly from `knowledge` gaps: `knowledge` means missing facts or information; `epistemic` means missing certainty-finding methods or perspective-rotation tools. An `epistemic` gap is flagged when an agent reports low confidence and no installed skill addresses the uncertainty type (perspective_blind, context_loss, conflict).

### skill-discovery-evaluate

1. Evaluate the candidate skill against format, quality, and safety criteria.
2. Validate the format by checking for YAML frontmatter, a valid name, a specific description, and the absence of deprecated markers.
3. Assess instruction quality to ensure steps are imperative, concrete, actionable, bounded in scope, and have clear trigger conditions.
4. Check Magna Carta compliance and system constraints, including user sovereignty (P1), affirmative consent (P2), generative space (P3), clear boundaries (P4), headless compliance, Regulation span validity, and crate path validity.
5. Score each of the 16 checks (4 format, 5 quality, 7 safety) from 0 to 2, where 0 is fail, 1 is partial, and 2 is pass. The maximum is 32.
6. The evaluate template emits `format_validation`, `quality_evaluation`, `safety_evaluation`, and `recommendation_reason`; combine with the `lisp_eval`-computed `overall_score` and `recommendation`.
7. Score every check without omitting any.
8. Compute `overall_score` and the recommendation in `lisp_eval`, never by hand. Bind `fmt`, `q` and `s` to the format, quality and safety score lists and `threshold` to the installable minimum:
   `(begin (define sum (lambda (xs) (if (= (length xs) 0) 0 (+ (car xs) (sum (cdr xs)))))) (let ((total (+ (sum fmt) (sum q) (sum s)))) (list total (if (member 0 s) "reject" (if (< total threshold) "revise" "install")))))`
   Tested: all 2s → `[32, install]`; one safety 0 → `reject`; all 1s at threshold 24 → `[16, revise]`. Any safety score of 0 rejects deterministically, including judgment checks such as P3. A format or quality 0 does NOT gate — it only diminishes the total (a candidate failing a format check can still reach `install` if the total clears 24 with no safety 0; verified live) — the safety checks are the gate.
9. The installable threshold is 24 of 32 (operator ruling 2026-09-26), keeping the roughly 73% bar that 16 of 22 set when the template miscounted 11 checks.

## Initial and target condition

- **Initial condition:** the task or gap description, its context, and the installed skill catalog (plus, for evaluate, the candidate skill's content).
- **Target condition:** route returns recommendations whose fit, floor and coverage band were recomputed in `lisp_eval`; evaluate returns a recommendation computed in `lisp_eval` from all 16 scores. Installation stays the operator's decision.

## Step types

| Step | Type | Oracle / critique |
|------|------|-------------------|
| Dimension scores, gap classification and impact, check scores | P | the operator, who decides installation |
| Composite fit, boost, floor, band; overall score and recommendation | D | the `lisp_eval` forms above |

## Pipeline

```mermaid
flowchart TD
    R[route<br/>rank installed skills] --> C{coverage?}
    C -- full --> X[invoke matched skill]
    C -- partial/none --> A[detect-gap<br/>classify uncovered capabilities]
    A --> B{existing skill closest?}
    B -- route_to_existing_skill --> R
    B -- create_skill / discover_external --> N[create-skill or external candidate]
    N --> F[evaluate<br/>score quality/safety]
    F --> H{installable?}
    H -- yes --> I[operator installs] --> R
    H -- no --> J[revise or reject]
```

Installation is an operator decision; this skill recommends, it never installs.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `skill-discovery-route.j2` | ROUTE — match a task or capability gap against the installed skill catalog. Scores each skill 0.0–1.0 (capability 0.50, description 0.25, trigger 0.25). Returns ranked recommendations with fit_score, match_reason, applicable templates, and invocation hints; emits `coverage_assessment` (full ≥0.8 / partial 0.4–0.79 / none <0.4) and `uncovered_capabilities` for detect-gap. Optional `epistemic_state` boosts certainty-finding skills in a low-confidence regime. |
| `skill-discovery-detect-gap.j2` | DETECT-GAP — classify gaps (coverage, feature, automation, knowledge, governance, quality, epistemic) and prioritize by impact. Recommends actions including `route_to_existing_skill`. |
| `skill-discovery-evaluate.j2` | EVALUATE — score a candidate skill against format, quality, and safety criteria (Magna Carta compliance, Regulation span validity) and produce a scored recommendation. |

To render a template, call the `render_template` tool with the template ref (e.g., `skill-discovery/skill-discovery-route`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `skill-discovery-route.j2`: `task_description`, `task_context`, `skill_catalog`, `max_recommendations`, `epistemic_state`
- `skill-discovery-detect-gap.j2`: `task_patterns`, `skill_catalog`
- `skill-discovery-evaluate.j2`: `candidate_skill_content`

## Regression case

All receipts executed live through `lisp_eval` (2026-10-01, batch-10 audit):

- Route composite `SD-0` (`scientific_gate_sd_0_` in `check-skill-lisp-forms.sh`, over the authored form): one boosted skill at 0.9/0.8/0.9 → fit 0.9 (the +0.20 clamps at 1), `full`; 0.5/0.5/0.5 → 0.5, `partial`; 0.3/0.2/0.1 → 0.225 below the floor, `none`; the same beside a 0.9/0.9/0.9 skill → `full` with the weak one ineligible; an empty list → `none`; a dimension of 1.2 → `malformed_scores` naming the skill; four recommendations against a cap of three → `too_many_recommendations`; a boost named for one skill lifts only that skill's trigger term.
- Evaluate, install: all 2s (16 checks) → `[32, "install"]`.
- Evaluate, safety-0 reject: one safety 0 with total 30 ≥ threshold 24 →
  `[30, "reject"]` — the safety gate dominates the total, deterministically.
- Evaluate, revise: all 1s → `[16, "revise"]` (below the 24-of-32 bar).

The skill's forms are executed at use time, never anchored in code.

## Constraints

- `skill-discovery-route.j2`: Evaluates every skill in the catalog — do not skip seemingly-irrelevant skills without scoring. fit_score and each dimension score are floats in [0.0, 1.0]. If coverage is `full`, `uncovered_capabilities` must be empty; if `none`, recommendations may be empty but `uncovered_capabilities` must be non-empty.
- `skill-discovery-detect-gap.j2`: Gap categories: coverage, feature, automation, knowledge, governance, quality, epistemic (7 categories). Input `skill_catalog` is the same array passed to route.
- `skill-discovery-evaluate.j2`: 16 checks scored 0–2; max score 32; min installable 24 (operator ruling 2026-09-26); safety 0 → reject.
- `lisp_eval` is available for deterministic scoring formulas (e.g., weighted combinations of quality, safety, and fit scores).
