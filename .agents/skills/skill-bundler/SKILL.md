---
shipped: false
name: skill-bundler
description: "Merge peer-level skill outputs into a grounded unified report, with a bounded correction if the merged report fails its target condition."
---

# Skill Bundler

Given peer-level skills' outputs on the same task, merge them into one report. The invoking agent obtains the outputs; `skill` retrieves instructions but does not execute a skill. This skill handles only the merge, not dispatch or composition. A satisfied merge closes after one pass; a measured gap allows one correction of the merge, never a rerun of the peer skills.

## When to Use

- You have run 3+ peer-level skills concurrently on the same task and need their outputs merged into one coherent report.
- You need per-skill summaries, cross-skill insights (what the combination reveals that no single skill did), explicit conflicts between skills' conclusions, and prioritized recommendations.
- You do NOT need skill composition, ordering resolution, or ontology anchoring — the bundler merges; it does not compose.

## When NOT to Use

- Composing or ordering skills — the bundler merges; it does not compose (its own constraint). Sequential skills with dependencies must run in order directly.
- A single skill's output — there is nothing to merge.
- Open-ended convergence or re-execution of peer skills — the bundler corrects its merge at most once.

## Instructions

### bundler-merge — bounded PDCA

1. **Plan (initial → target).** Record the initial condition: requested task, ordered `skill_names`, the corresponding `skill_outputs`, and explicit error states. Target condition: exactly one attributable summary for every named skill (including errors), no unsupported findings, and cross-skill insights and recommendations referencing only successful inputs. An absent output is not an errored output. Before rendering, call `lisp_eval` with `(= (length skill_names) (length skill_outputs))` and those arrays as env bindings. If false, stop and request the missing output or correct the input pairing; never synthesize a missing result.
2. **Do.** Render `skill-bundler/bundler-merge` using the paired inputs. For each skill, write 2-3 sentences capturing its key findings, verdict, or recommendations. Label each with the skill name. Every skill gets a summary, even if it errored.
3. **Do.** Identify points where successful outputs complement, contradict, or build on each other. Only include insights that require looking at 2+ successful skills together. Surface contradictory conclusions explicitly. Prioritize actionable recommendations and cite which successful skill(s) informed them. Write one cohesive report rather than stapling outputs together.
4. **Check.** Compare the produced report with the original pairs. Call `lisp_eval` on the ordered skill names and extracted report summary names: `(begin (define same-names (lambda (a b) (if (= (length a) 0) (= (length b) 0) (if (= (length b) 0) nil (and (string= (car a) (car b)) (same-names (cdr a) (cdr b))))))) (and (same-names skill_names summary_names) (= (length unsupported_names) 0)))`. Inspect every substantive finding against its source output and mark unsupported findings as gaps; names and counts cannot prove semantic grounding. Check that error entries remain identified and do not contribute findings.
5. **Act.** If every check passes, return the report after this one pass. If a merge-only gap remains, revise the report once using the named gap and recheck against the *same* inputs. If it still fails, stop with the report marked incomplete and the remaining gaps; never invent input data or re-run peer skills to make the merge look complete.

### Step types and reference model

| Step | Type | Oracle / critique |
|------|------|-------------------|
| 1 Pairing check, 4 name/order check | D | the pinned `lisp_eval` forms |
| 2–3 Summaries, cross-skill insights | P | step 4's per-finding source inspection; the operator |
| 5 One correction | P | the same step-4 checks against the same inputs |

There is no external reference model: correctness is governed by the pinned forms and per-finding source inspection, not by a summarization method.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `bundler-merge.j2` | Merge the outputs of N concurrently-executed skills into a single cohesive report with per-skill summaries, cross-skill insights, conflict surfacing, and prioritized recommendations. |

To render a template, call the `render_template` tool with the template ref (e.g., `skill-bundler/bundler-merge`) and a context object with the required variables.

## Constraints

- `bundler-merge.j2`: Public.
- The local PDCA corrects the merged report at most once; it does not rerun peer skills or optimize the skill itself. Outcome evaluation and any skill change belong to the operator's algedonic-review gemba walk.
- Do not invent findings that no skill produced.
- Do not omit a skill from the summaries — every skill gets a summary, even if it errored.
- The merged report must reference each skill by name at least once.
- Keep the merged report under 2000 words.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
