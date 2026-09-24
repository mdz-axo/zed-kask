---
name: skill-logic-audit
core: true
description: "Goal- and callsite-grounded audit of .j2 templates and legacy manifests. Compares distinct candidate designs on fixed tasks and files a proposal for the operator's algedonic review; never edits its target or treats a formal check as proof of prompt quality."
---

# Skill Logic Audit

Bounded logic audit of .j2 templates against their annotated goals (and legacy manifest.yaml files as inert artifacts, not runtime skill definitions). Unfolded from skill-maintenance (originally folded 2026-07-25, unfolded 2026-08-14).

## The composition law (context for every audit)

A kask skill's core process — including at least one PDCA
(Plan→Do→Check→Act) self-improvement loop — lives in the SKILL.md body.
Templates are leaves: steps of that loop, rendered by `render_template`
at the points the SKILL.md directs. A template may contain its own
internal loop, but it is never the carrier of the skill's core loop.
Audit a template as a step-leaf: its goal must serve the SKILL.md phase
that invokes it, its inputs must match what that phase passes, and its
outputs must feed the phase that consumes them.

## When to Use

- Auditing a .j2 template's logic against its stated `{# goal: ... #}` annotation
- Auditing an explicitly requested legacy manifest.yaml's annotated text, without treating it as a live skill contract
- Comparing a template's existing design with materially different alternatives before proposing a change
- Packaging a comparison-backed template change as a proposal for the operator's gemba-walk decision

## When NOT to Use

- SKILL.md bodies — not valid audit targets (this skill's own constraint); use `skill-maintenance`.
- Skill health scoring, staleness signals, retirement thresholds — `skill-maintenance-audit`.
- Coverage-gap mapping against the corpus — `skill-maintenance-coverage`.

## Instructions

### logic-load-goal — Plan

1. Read the target via `read_file`; parse its `{# goal: ... #}` (.j2) or `# goal: ...` (explicitly requested legacy manifest) annotation, preserving the exact goal text. If missing, report an unauditable target; do not invent a goal.
2. For a live .j2, read the invoking SKILL.md phase and the inputs supplied to, and outputs consumed from, the template. An annotation alone is not the objective: record any mismatch with the canonical phase. A manifest is inert; assess only its stated textual goal, never execution behavior.
3. Before generating revisions, fix the acceptance cases: at least one representative success, one failure/negative case and, for a live template, one downstream handoff. State expected observable outputs, hard contracts (including human approval), costs to compare, and the same evaluation method for all candidates. If execution fixtures or a trustworthy evaluator are unavailable, mark behavioral comparison `unverified` rather than inventing results.

### logic-critique-template — Do

1. Render `logic-critique-template` with the goal, target and invoking phase/acceptance cases. Cite concrete defects against the goal AND the phase; reject style-only concerns. Use `kask/scripts/audit/skill-corpus-prescreen.sh` and `skill-corpus-contract-audit.sh` for shape leads, not as proof of usefulness.
2. Render `logic-critique-critique` to discard unsupported concerns. Generate at most four **distinct** candidates: unchanged baseline, localized repair, subtraction/simplification, and a replacement structure not derived by editing the baseline. If a category is inapplicable, explain why; never force a modification. Keep SKILL.md process changes out of this audit and route them to `skill-maintenance`.

### logic-compare-candidates — Check → Act

1. Render `logic-compare-candidates` with the fixed cases, candidate artifacts and actual observations. Run the same cases on baseline and candidates with the available prompt/skill harness; separately check rendering, contract and handoff shape. Evaluate semantic correctness against the predeclared expected behavior or independent human judgments, **not** the candidate's own critique or a goal-overlap score. Include cost and regression cases. No runnable oracle means an unverified proposal, not a measured improvement.
2. Call `lisp_eval` on structured case/candidate records for completeness, counts, hard-gate status and score arithmetic. Supply actual recorded outcomes; a pass from agent-supplied labels does not validate the labels. Reconcile case IDs and missing observations with the underlying logs; any missing, failed hard gate or unverified behavioral result prevents an improvement claim.
3. If and only if a concrete decision rule needs mathematical assurance, invoke `lean-prover` for its **exact formal statement** (e.g. selection never permits an unapproved edit); inspect compilation, axioms and a negative control, then test the modeled rule's implementation. Lean does not prove that a prompt is good or globally optimal. If Lean is unavailable, label the formal claim unverified; do not mislabel a `lisp_eval` result as a proof.
4. Name a candidate for proposal only if it clears every hard contract, improves the fixed outcome measure against the baseline, and has no unacceptable regression. Otherwise propose nothing; report evidence gaps. Re-enter candidate generation at most once for a newly discovered falsifier, using the same held-out cases; then stop and report the observed frontier, not an absolute optimum. This audit measures and proposes; it never decides.

### logic-compose-proposal — file for the algedonic review

1. If a candidate qualifies, render `logic-compose-proposal` with its actual content and comparison evidence and produce its full unified diff. It must be the simplest passing candidate that attains the better measured outcome. If baseline wins or results are inconclusive, report that and leave the artifact unchanged.
2. Recheck goal annotation, `[inference]` contract and rendering for the proposed .j2. Never claim a legacy manifest edit changes skill execution.
3. Write the proposal (target path, goal, diff, comparison evidence, open falsifiers) via `terminal` to `~/Documents/zk-data/curator/proposals/{skill}/{date}-{run}.json` and stop. Do not edit the target. The operator accepts, rejects or counters it in `algedonic-review`'s gemba walk (operator ruling 2026-09-24: skill evaluation is separated from execution). An accepted proposal is applied afterwards by checking that the diff still matches the current file; a drifted file goes back to the review.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `logic-load-goal.j2` | Parse the annotated goal: block from a .j2 or manifest.yaml file and return it as a normalized string. Verify that a goal exists and is non-empty. |
| `logic-critique-template.j2` | Adversarial critique of a template against its annotated goal and invoking SKILL.md phase, grounded in fixed success, failure and handoff cases; locate each material defect. |
| `logic-critique-critique.j2` | Review a critique for soundness and goal-anchoring. Separate valid goal-anchored concerns from spurious ones. |
| `logic-compare-candidates.j2` | Compare the unchanged template and distinct candidate designs against fixed success, failure and handoff cases using observed evidence; reject regressions and retain the baseline when no verified improvement wins. |
| `logic-compose-proposal.j2` | Compose a comparison-backed proposed artifact and unified diff from calibrated concerns for the algedonic review, or retain the baseline when evidence does not justify an edit. |

To render a template, call the `render_template` tool with the template ref (e.g., `skill-logic-audit/logic-load-goal`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `logic-load-goal.j2`: `target_path`,`target_content`
- `logic-critique-template.j2`: `goal`,`target_path`,`target_content`,`template_type`,`invoking_phase`,`acceptance_cases`
- `logic-compare-candidates.j2`: `goal`,`invoking_phase`,`acceptance_cases`,`candidates`,`observations`
- `logic-compose-proposal.j2`: `goal`,`target_path`,`original_content`,`valid_concerns`,`comparison`,`winning_content`,`user_counter_proposal`

## Constraints

- `logic-load-goal.j2`: Operates on .j2 templates and .yaml manifests ONLY. SKILL.md files are NOT valid audit targets.
- `logic-critique-template.j2`: Be adversarial but grounded. Reject purely stylistic complaints that do not affect logical efficiency or correctness.
- `logic-critique-critique.j2`: A concern is valid only if it explicitly links a concrete template defect to the goal.
- `logic-compose-proposal.j2`: Propose only a comparison-backed candidate; a larger redesign is allowed when the baseline and smaller candidates fail the fixed tasks.
- Only the operator's decision in the algedonic review accepts, rejects, or counters a proposal. This audit never edits its target; `user_counter_proposal` carries a counter the operator gave in the review.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
