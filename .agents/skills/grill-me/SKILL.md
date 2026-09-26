---
name: grill-me
description: "Socratic interrogation skill. Tests deep understanding through escalating difficulty (Recall → Mechanism → Rationale → Edge Cases → Synthesis). Probes gaps, challenges assumptions, produces gap analysis.
"
---

# Grill Me

Socratic interrogation skill. Tests deep understanding through escalating difficulty (Recall → Mechanism → Rationale → Edge Cases → Synthesis). Probes gaps, challenges assumptions, produces gap analysis.


## Reference model

Socratic questioning, with a difficulty ladder (Recall → Mechanism → Rationale → Edge cases → Synthesis) in the manner of Bloom's taxonomy (Bloom et al., 1956). Initial condition: calibrate mode's baseline. Target condition: level-5 completion or round 5, then an assessment built only from actual answers.

## When to Use

- When testing deep understanding of a topic through Socratic interrogation with escalating difficulty (Recall → Mechanism → Rationale → Edge Cases → Synthesis).
- When calibrating a user's baseline knowledge on a specific topic.
- When dynamically adapting question difficulty based on answer quality ratios.
- When probing for knowledge gaps and challenging assumptions during an oral examination.
- When synthesizing a final gap analysis with per-area ratings and prioritized study recommendations.

## When NOT to Use

- Teaching new material — grill-me tests understanding that exists; it does not deliver a lesson.
- Reviewing a code change — use `code-review`.
- Adversarial self-review of one's own reasoning — use `falsifiability`'s challenge stage; grill-me interrogates a learner about a topic, decoupled from authorship.

## Instructions

### grill-me-round

1. Conduct a rigorous oral examination on the specified topic.
2. Generate 2-3 questions at the current difficulty level, adhering to the question taxonomy (Recall, Mechanism, Rationale, Edge Cases, Synthesis).
3. If mode is "calibrate", assess the user's baseline knowledge directly and precisely.
4. If mode is "interrogate", grade each previous answer Solid, Partial or Gap (P). The level and the hold/reprobe direction come from the Feedback gate below, not from this step.
5. Track attempts per question across rounds; the Feedback gate retires a question after 3 failed attempts, and this step explains its answer instead of re-asking it.
6. Maintain a direct, sharp tone akin to a demanding technical interviewer, using specific challenging phrases without being mean-spirited.
7. Give minimal hints if requested, without solving the questions for the user.
8. Output a JSON object containing questions, evaluations, current level; each evaluation carries its Solid/Partial/Gap rating.

### grill-me-assess

1. Synthesize a comprehensive final knowledge assessment for the specified topic.
2. Rate each knowledge area as Solid, Partial, or Gap based on all answers and running assessment.
3. Formulate specific study recommendations, prioritized by impact with the most critical gaps first.
4. Do not sugarcoat gaps, but avoid being demoralizing.
5. Output a JSON object containing the summary, recommendations, and overall assessment.

### Feedback gate (escalation is D — `lisp_eval`)

Grading each answer Solid / Partial / Gap is P (the round render's judgment, critiqued by the learner's next answer and by the operator). The escalation decision over those grades is a fixed rule, computed with `lisp_eval`, never judged:

1. After each answer, compute `solid_ratio` = Solid answers / answered questions this round (0 answered → hold), then call `lisp_eval`:
   - form: `(cond ((= answered 0) (list level "hold")) ((>= round 5) (list level "complete")) ((>= solid_ratio 0.8) (if (>= level 5) (list 5 "complete") (list (+ level 1) "escalate"))) ((>= solid_ratio 0.4) (list level "hold")) (t (list level "reprobe")))`
   - env: `{ "level": <current level 1–5>, "solid_ratio": <ratio>, "answered": <answered questions this round>, "round": <round number> }`
   Wait for the answer before counting it; rendering a question is not evaluation.
2. Retire questions after 3 failed attempts with `lisp_eval` over the per-question attempt counts: `(begin (define retire (lambda (a) (if (is_null a) (quote ()) (if (>= (car (cdr (car a))) 3) (cons (car (car a)) (retire (cdr a))) (retire (cdr a)))))) (retire attempts))`, env `{ "attempts": [[<question id>, <failed attempts>], ...] }`. Pass the retired ids to the next round render so they are explained, not re-asked.
2. Stop at level-5 completion or round 5, whichever comes first, and run `grill-me-assess` on actual answers. Never invent a rating for unanswered questions; report gaps as unassessed where evidence is absent.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `grill-me-round.j2` | Generate interrogation questions at the supplied difficulty level and grade each answer; the Feedback gate computes escalation. |
| `grill-me-assess.j2` | Synthesize final assessment with per-area ratings (Solid/Partial/Gap) and prioritized study recommendations. |


To render a template, call the `render_template` tool with the template ref (e.g., `grill-me/grill-me-round`) and a context object with the required variables.

## Constraints

- `grill-me-round.j2`: Public.
- `grill-me-assess.j2`: Public.
- Escalation is the `lisp_eval` rule in the Feedback gate; there is no escalation template.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
