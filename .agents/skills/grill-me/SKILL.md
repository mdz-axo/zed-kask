---
name: grill-me
description: "Socratic interrogation skill. Tests deep understanding through escalating difficulty (Recall → Mechanism → Rationale → Edge Cases → Synthesis). Probes gaps, challenges assumptions, produces gap analysis."
---

# Grill Me

Socratic interrogation skill. Tests deep understanding through escalating difficulty (Recall → Mechanism → Rationale → Edge Cases → Synthesis). Probes gaps, challenges assumptions, produces gap analysis.


## Reference model

Socratic questioning (Paul & Elder, *The Art of Socratic Questioning*, 2006 → `onto_anchor` `derived:socratic_method`), with a local 5-level adaptation (Recall → Mechanism → Rationale → Edge Cases → Synthesis) of Bloom's taxonomy (Bloom et al., 1956; Krathwohl, 2002 `doi:10.1207/s15430421tip4104_2` → `onto_anchor` `derived:bloom_taxonomy`) and adaptive difficulty scaffolding across the Zone of Proximal Development (Vygotsky, 1978 → `onto_anchor` `derived:zone_of_proximal_development`). The five-level ladder is not the revised taxonomy's six categories. The thresholds, round bound and retirement policy below are project rules, not claims from those publications. Existing corpus carrier retrieval does not establish primary-book inclusion or complete method fidelity.

Initial condition: a topic, an empty question/answer/grading ledger and a calibration round. Target condition: level-5 completion or round 5, then an assessment grounded in actual answers, with unanswered areas explicitly Unassessed. Failed identity/grade gates stop admission; repair the named malformed batch once and recheck, then report the gap rather than fabricate evidence. The five-round bound cannot reset on a repair or regrade.

## D/P labelling

Question generation and answer grading (Solid/Partial/Gap) are P — the round
render's judgment, critiqued by the learner's next answer and the operator.
Identity assignment, grade-batch admission, latest-grade joins, distinct failed-answer counts, retirement, feedback and final evidence-reference checks are D: the authored `lisp_eval` forms below own them. Question drafting, semantic grading, initial calibrated level and final synthesis remain P. Rendering does not execute the prompt. The caller executes each rendered phase, validates its output, then invokes the next phase; no model-emitted count or positional join supplies a gate.

## When to Use

- When testing deep understanding of a topic through Socratic interrogation with escalating difficulty (Recall → Mechanism → Rationale → Edge Cases → Synthesis).
- When calibrating a user's baseline knowledge on a specific topic.
- When dynamically adapting question difficulty based on answer quality ratios.
- When probing for knowledge gaps and challenging assumptions during an oral examination.
- When synthesizing a final gap analysis with per-area ratings and prioritized study recommendations.

## When NOT to Use

- Teaching new material — grill-me tests understanding that exists; it does not deliver a lesson.
- Reviewing a code change — use `code-review`.
- Adversarial self-review of one's own reasoning — use `falsifiability`'s eliminative inference (the admissibility gate and the discriminating tests challenge your own hypotheses); grill-me interrogates a learner about a topic, decoupled from authorship.

## Instructions

### 1. Own the raw records (D)

Keep one session ledger; do not create a Rust controller or a parallel persistent store:

- `question_bank`: unique `{id, question, level, area}` rows. GM-1 assigns integer IDs to new questions from round/slot; a retry retains its original ID and text. Append only newly assigned IDs, not retry copies.
- `answers`: unique `{id, question_id, text, round}` rows from actual learner replies. Allocate the next integer answer ID with `lisp_eval` `(+ (length answers) 1)`; append once per distinct submitted answer, never once per grading. A new submission, even identical text, is a distinct answer; a regrade reuses its existing ID. Never attach a reply by array position or infer an answer to a different question.
- `grade_events`: append accepted `{answer_id, rating, note}` rows in execution order. Ratings are exactly `Solid`, `Partial`, `Gap`. Regrading appends an event for the same answer ID. The latest event supplies current feedback/assessment; distinct answers with any accepted Gap event supply cumulative failed-attempt history. This retains the existing monotonic failure-history policy: a later grade correction does not erase a recorded failed attempt, but cannot add a second one for that answer (operator approval 2026-10-20, proposal `3568842d`).

Start with all three arrays empty, `round=1`, `level=1`, `mode="calibrate"`. Track running semantic `assessment` separately; it is not an authoritative counter. Keep the calibration's actual graded output separately as `calibration`, null until answers have been graded. Retain these records as the examination transcript; list its artifact location if persisted, otherwise state that it remains in the conversation.

### 2. Ask → wait → grade → gate (D/P)

1. Before asking, run GM-STATE below. If any actual current answer is ungraded, grade it first. Do not generate another batch while awaiting learner replies; advance a round only after the current batch has been answered or explicitly skipped by the learner. Skip is not an answer or a grade.
2. Render `grill-me/grill-me-round`, `phase="ask"`, and execute its question-drafting instructions. GM-1 validates/stamps its `question_drafts` and reconciles `retired_explanations` to the computed retired IDs; display only the admitted question records and explanations. It admits 2–3 questions, preserves legitimate retries, and refuses retired IDs. A new paraphrase of a retired question is also prohibited; detecting that semantic equivalence remains P, not a claim made by the ID check.
3. Wait for actual answers, append their identified rows, and rerun GM-STATE. Select the requested answer records by their IDs from its `answer_records`, not by reconstructing their text. Normally grade `ungraded_answer_ids`; an explicit regrade request selects existing IDs instead. Never allocate another answer ID for a regrade.
4. Render the same template with `phase="grade"` and only those `answers_to_grade`. Execute grading, then run GM-G with their exact `answer_ids` and the returned `evaluations`. False or a tool error blocks the batch; do not append partial/malformed grades. For calibration, accept `calibrated_level` only from this answered grade phase and seed the first interrogate level from it. In interrogate mode the caller supplies null to GM-G; the grader does not echo that constant. Ask output contains question drafts and, when needed, semantic retired-question explanations, not copied assessment/empty grading fields; the caller retains inactive-phase state directly. Ask phases cannot calibrate from unasked/unanswered questions.
5. Append accepted evaluations to `grade_events`, update the semantic assessment, then run GM-STATE. In interrogate mode use its returned `next_level` and `direction`, not a model-typed ratio. After the calibration batch, seed the first interrogate level with `calibrated_level` without another escalation; still honor GM-STATE's pending-grade and stop checks. On `await_grades`, return to step 4. On `stop`, assess; otherwise advance the round (at most five question batches total), use mode `interrogate`, and return to step 1. Calibration consumes the first batch. A zero-answer batch holds; at round 5 it still ends the examination. A regrade never resets/increments the round or generates questions on its own.
6. At completion or a learner stop, render `grill-me/grill-me-assess` with GM-STATE's joined `answer_records`, the question bank, calibration and running assessment. Execute synthesis and validate its raw `summary` through GM-2. Keep recommendations and grading semantic; GM-2 checks vocabulary, area coverage and actual evidence references, not whether a judgment is wise. Empty/unanswered areas must be `Unassessed`, not `Gap`.

Copyable `render_template.context` shapes (JSON-encode the object as the outer tool's context string; substitute complete recorded values, never stringify the inner arrays):

`grill-me/grill-me-round` — ask or grade uses the same required fields; unused arrays remain explicit `[]`:
```json
{
  "topic": "<learner topic>", "phase": "ask", "mode": "calibrate",
  "current_level": 1, "round_number": 1,
  "question_bank": [], "answers_to_grade": [], "assessment_so_far": {},
  "gate_result": {"direction": "hold", "retired_ids": [], "stop": false}
}
```
For grade, set `phase="grade"`, preserve topic/mode/level/round/bank/assessment and replace `answers_to_grade` with the exact selected GM-STATE records. For subsequent ask, pass the actual GM-STATE result and its level/direction; never supply a fabricated pass object.

`grill-me/grill-me-assess`:
```json
{
  "topic": "<learner topic>", "question_bank": [],
  "answer_records": [], "assessment": {}, "calibration": null
}
```

### GM-G — admit one complete semantic grading batch (D)

Env: `{"answer_ids": [1,2], "evaluations": [{"answer_id":2,"rating":"Solid","note":"..."},{"answer_id":1,"rating":"Gap","note":"..."}], "mode":"interrogate", "calibrated_level":null}`. IDs come from the actual selected answer records. Out-of-order grading is valid; missing, duplicate, extra or invented IDs are not.

```lisp
(begin
  (define gm-grade-batch (lambda (ids rows)
    (if (is_null rows) (is_null ids)
      (let ((row (car rows)) (id (assoc! "answer_id" (car rows))))
        (and (numberp id) (eq (- id id) 0) (member id ids)
          (member (assoc! "rating" row) (list "Solid" "Partial" "Gap"))
          (stringp (assoc! "note" row))
          (gm-grade-batch (remove-id ids id) (cdr rows)))))))
  (define remove-id (lambda (ids id)
    (if (is_null ids) (list)
      (if (= (car ids) id) (cdr ids) (cons (car ids) (remove-id (cdr ids) id))))))
  (define ids-ok (lambda (ids seen)
    (if (is_null ids) true
      (and (numberp (car ids)) (eq (- (car ids) (car ids)) 0) (> (car ids) 0)
        (not (member (car ids) seen)) (ids-ok (cdr ids) (cons (car ids) seen))))))
  (and (listp answer_ids) (listp evaluations) (> (length answer_ids) 0)
    (ids-ok answer_ids (list)) (= (length answer_ids) (length evaluations))
    (gm-grade-batch answer_ids evaluations)
    (cond ((string= mode "interrogate") (is_null calibrated_level))
      ((string= mode "calibrate")
        (and (numberp calibrated_level) (eq (- calibrated_level calibrated_level) 0)
          (>= calibrated_level 1) (<= calibrated_level 5)))
      (true false))))
```

### GM-STATE — derive feedback and retirement from raw records (D)

Env is exactly `{"question_bank":[], "answers":[], "grade_events":[], "level":1, "round":1}` with the current ledger substituted. Unknown/duplicate IDs, invalid grades or out-of-bound levels/rounds return `invalid_state`; missing fields/type errors are tool refusals. No such result permits progression. Objects/arrays share Lisp's list representation: these checks are not full JSON Schema validation.

```lisp
(begin
  (define integer-between (lambda (x lo hi)
    (and (numberp x) (eq (- x x) 0) (>= x lo) (<= x hi))))
  (define text-ok (lambda (x) (and (stringp x) (> (length x) 0))))
  (define find-id (lambda (rows id)
    (if (is_null rows) nil (if (= (assoc! "id" (car rows)) id) (car rows) (find-id (cdr rows) id)))))
  (define unique-ids (lambda (rows seen)
    (if (is_null rows) true
      (let ((id (assoc! "id" (car rows))))
        (and (integer-between id 1 9223372036854775807) (not (member id seen))
          (unique-ids (cdr rows) (cons id seen)))))))
  (define questions-ok (lambda (rows)
    (if (is_null rows) true
      (and (text-ok (assoc! "question" (car rows))) (text-ok (assoc! "area" (car rows)))
        (integer-between (assoc! "level" (car rows)) 1 5) (questions-ok (cdr rows))))))
  (define answers-ok (lambda (rows)
    (if (is_null rows) true
      (and (integer-between (assoc! "question_id" (car rows)) 1 9223372036854775807)
        (not (is_null (find-id question_bank (assoc! "question_id" (car rows)))))
        (text-ok (assoc! "text" (car rows)))
        (integer-between (assoc! "round" (car rows)) 1 round) (answers-ok (cdr rows))))))
  (define events-ok (lambda (rows)
    (if (is_null rows) true
      (and (integer-between (assoc! "answer_id" (car rows)) 1 9223372036854775807)
        (not (is_null (find-id answers (assoc! "answer_id" (car rows)))))
        (member (assoc! "rating" (car rows)) (list "Solid" "Partial" "Gap"))
        (stringp (assoc! "note" (car rows))) (events-ok (cdr rows))))))
  (define latest (lambda (rows id)
    (if (is_null rows) nil
      (let ((later (latest (cdr rows) id)))
        (if (not (is_null later)) later
          (if (= (assoc! "answer_id" (car rows)) id) (car rows) nil))))))
  (define failed (lambda (rows id)
    (if (is_null rows) false
      (or (and (= (assoc! "answer_id" (car rows)) id) (string= (assoc! "rating" (car rows)) "Gap"))
        (failed (cdr rows) id)))))
  (define failures (lambda (rows qid)
    (if (is_null rows) 0
      (+ (if (and (= (assoc! "question_id" (car rows)) qid) (failed grade_events (assoc! "id" (car rows)))) 1 0)
        (failures (cdr rows) qid)))))
  (define attempts (lambda (rows)
    (if (is_null rows) (list)
      (cons (list (assoc! "id" (car rows)) (failures answers (assoc! "id" (car rows)))) (attempts (cdr rows))))))
  (define retired (lambda (rows)
    (if (is_null rows) (list)
      (if (>= (nth 1 (car rows)) 3) (cons (car (car rows)) (retired (cdr rows))) (retired (cdr rows))))))
  (define records (lambda (rows)
    (if (is_null rows) (list)
      (let ((a (car rows)) (g (latest grade_events (assoc! "id" (car rows))))
            (q (find-id question_bank (assoc! "question_id" (car rows)))))
        (cons (list (list "id" (assoc! "id" a)) (list "question_id" (assoc! "id" q))
          (list "question" (assoc! "question" q)) (list "area" (assoc! "area" q))
          (list "text" (assoc! "text" a)) (list "round" (assoc! "round" a))
          (list "rating" (if (is_null g) nil (assoc! "rating" g)))
          (list "note" (if (is_null g) nil (assoc! "note" g)))) (records (cdr rows)))))))
  (define current (lambda (rows)
    (if (is_null rows) (list)
      (if (= (assoc! "round" (car rows)) round) (cons (car rows) (current (cdr rows))) (current (cdr rows))))))
  (define pending (lambda (rows)
    (if (is_null rows) (list)
      (if (is_null (assoc! "rating" (car rows))) (cons (assoc! "id" (car rows)) (pending (cdr rows))) (pending (cdr rows))))))
  (define solids (lambda (rows)
    (if (is_null rows) 0 (+ (if (string= (assoc! "rating" (car rows)) "Solid") 1 0) (solids (cdr rows))))))
  (define gm-state (lambda ()
    (if (not (and (integer-between level 1 5) (integer-between round 1 5)
      (listp question_bank) (listp answers) (listp grade_events)
      (unique-ids question_bank (list)) (unique-ids answers (list))
      (questions-ok question_bank) (answers-ok answers) (events-ok grade_events))) "invalid_state"
      (let ((joined (records answers)) (counts (attempts question_bank)))
        (let ((batch (current joined)))
          (let ((n (length batch)) (s (solids batch)) (missing (pending batch)))
            (let ((direction (cond ((> (length missing) 0) "await_grades") ((= n 0) "hold")
              ((>= round 5) "complete") ((>= (* s 5) (* n 4)) (if (= level 5) "complete" "escalate"))
              ((>= (* s 5) (* n 2)) "hold") (true "reprobe"))))
              (list (list "answer_records" joined) (list "attempt_counts" counts)
                (list "retired_ids" (retired counts)) (list "ungraded_answer_ids" missing)
                (list "answered_this_round" n) (list "solid_this_round" s)
                (list "next_level" (if (string= direction "escalate") (+ level 1) level))
                (list "direction" direction) (list "stop" (or (>= round 5) (string= direction "complete")))))))))))
  (gm-state))
```

### GM-1 — validate question drafts and assign identities (D)

Env: `{"drafts":[], "explanations":[], "question_bank":[], "retired_ids":[], "level":1, "round":1}`. Supply `drafts` and `explanations` from the ask output (`question_drafts`, `retired_explanations`); only when the computed retired set is empty does the caller bind explanations to `[]` without a model echo. Other fields come from the current ledger/GM-STATE. New IDs are `10*round + slot` (slots 1–3); retries return the original question record. `invalid_questions` or tool errors block delivery.

```lisp
(begin
  (define lookup (lambda (rows id)
    (if (is_null rows) nil (if (= (assoc! "id" (car rows)) id) (car rows) (lookup (cdr rows) id)))))
  (define has-question (lambda (rows text)
    (if (is_null rows) false (or (string= (assoc! "question" (car rows)) text) (has-question (cdr rows) text)))))
  (define draft-ok (lambda (d slot)
    (let ((retry (assoc! "retry_id" d)))
      (if (is_null retry)
        (and (stringp (assoc! "question" d)) (> (length (assoc! "question" d)) 0)
          (stringp (assoc! "area" d)) (> (length (assoc! "area" d)) 0)
          (not (has-question question_bank (assoc! "question" d)))
          (is_null (lookup question_bank (+ (* round 10) slot))))
        (and (numberp retry) (eq (- retry retry) 0) (not (is_null (lookup question_bank retry)))
          (not (member retry retired_ids)) (= (assoc! "level" (lookup question_bank retry)) level)
          (is_null (assoc! "question" d)) (is_null (assoc! "area" d)))))))
  (define build (lambda (rows slot seen texts)
    (if (is_null rows) (list)
      (let ((d (car rows)))
        (if (not (draft-ok d slot)) "invalid_questions"
          (let ((q (if (is_null (assoc! "retry_id" d))
            (list (list "id" (+ (* round 10) slot)) (list "question" (assoc! "question" d))
              (list "level" level) (list "area" (assoc! "area" d)))
            (lookup question_bank (assoc! "retry_id" d)))))
            (if (or (member (assoc! "id" q) seen) (member (assoc! "question" q) texts)) "invalid_questions"
              (let ((rest (build (cdr rows) (+ slot 1) (cons (assoc! "id" q) seen) (cons (assoc! "question" q) texts))))
                (if (stringp rest) rest (cons q rest))))))))))
  (define remove-retired (lambda (ids id)
    (if (is_null ids) (list)
      (if (= (car ids) id) (cdr ids) (cons (car ids) (remove-retired (cdr ids) id))))))
  (define explanations-ok (lambda (rows remaining)
    (if (is_null rows) (is_null remaining)
      (let ((id (assoc! "question_id" (car rows))) (text (assoc! "explanation" (car rows))))
        (and (numberp id) (eq (- id id) 0) (member id remaining)
          (not (is_null (lookup question_bank id))) (stringp text) (> (length text) 0)
          (explanations-ok (cdr rows) (remove-retired remaining id)))))))
  (define gm-question-plan (lambda ()
    (if (and (listp explanations) (explanations-ok explanations retired_ids)
      (numberp round) (eq (- round round) 0) (>= round 1) (<= round 5)
      (numberp level) (eq (- level level) 0) (>= level 1) (<= level 5)
      (listp drafts) (>= (length drafts) 2) (<= (length drafts) 3))
      (build drafts 1 (list) (list)) "invalid_questions")))
  (gm-question-plan))
```

### GM-2 — validate assessment evidence, not semantic quality (D)

Env: `{"question_bank":[], "answer_records":[], "summary":[]}`; records are GM-STATE's exact output and `summary` is the final synthesis. Every asked area must occur once. Graded summaries reference actual graded answer IDs in that area; areas without graded evidence are `Unassessed` with `answer_ids: []`.

```lisp
(begin
  (define find-answer (lambda (rows id)
    (if (is_null rows) nil (if (= (assoc! "id" (car rows)) id) (car rows) (find-answer (cdr rows) id)))))
  (define area-exists (lambda (rows area)
    (if (is_null rows) false (or (string= (assoc! "area" (car rows)) area) (area-exists (cdr rows) area)))))
  (define graded-area (lambda (rows area)
    (if (is_null rows) false
      (or (and (string= (assoc! "area" (car rows)) area) (not (is_null (assoc! "rating" (car rows)))))
        (graded-area (cdr rows) area)))))
  (define refs-ok (lambda (ids area seen)
    (if (is_null ids) true
      (and (numberp (car ids)) (eq (- (car ids) (car ids)) 0) (not (member (car ids) seen))
        (not (is_null (find-answer answer_records (car ids))))
        (not (is_null (assoc! "rating" (find-answer answer_records (car ids)))))
        (string= (assoc! "area" (find-answer answer_records (car ids))) area)
        (refs-ok (cdr ids) area (cons (car ids) seen))))))
  (define rows-ok (lambda (rows seen)
    (if (is_null rows) true
      (let ((r (car rows)) (area (assoc! "area" (car rows))))
        (and (stringp area) (not (member area seen)) (area-exists question_bank area)
          (stringp (assoc! "notes" r)) (listp (assoc! "answer_ids" r))
          (if (string= (assoc! "rating" r) "Unassessed")
            (and (is_null (assoc! "answer_ids" r)) (not (graded-area answer_records area)))
            (and (member (assoc! "rating" r) (list "Solid" "Partial" "Gap"))
              (> (length (assoc! "answer_ids" r)) 0) (refs-ok (assoc! "answer_ids" r) area (list))))
          (rows-ok (cdr rows) (cons area seen)))))))
  (define coverage (lambda (rows)
    (if (is_null rows) true
      (and (area-exists summary (assoc! "area" (car rows))) (coverage (cdr rows))))))
  (define gm-assessment (lambda ()
    (and (listp summary) (rows-ok summary (list)) (coverage question_bank))))
  (gm-assessment))
```

## Registry Templates

| Template | Purpose |
|----------|---------|
| `grill-me-round.j2` | Draft questions OR grade identified actual answers; a deterministic gate separates the phases. |
| `grill-me-assess.j2` | Synthesize evidence-linked final area ratings and recommendations; unanswered areas are Unassessed. |

## Regression case

`check-skill-lisp-forms.sh` parses these authored forms; run `bash kask/scripts/cargo-test-nonzero.sh -p hkask-lisp --bin check-skill-lisp-forms grill_` for their decision regressions against production evaluation, not Rust copies. Pin repeated Gap grading of one answer (one failure), latest-grade feedback, three distinct failed answers (retirement), exact 80%/40% boundaries, no answers, pending grades, level/round bounds, unknown/duplicate IDs, lowercase grades, out-of-order complete grade batches, missing batches, legal retry versus retired retry, new question IDs, complete retired-answer explanation handoffs and evidence-linked/Unassessed final areas. Registry-render tests pin grade/ask separation and record delivery. These are finite computation/handoff checks, not evidence of improved learner outcomes or primary-source fidelity.

## Constraints

- Never let a regrade create an answer or reset a question/answer ID, round, failure history or loop bound.
- Never deliver a next question before the preceding grades pass GM-G and GM-STATE; hints do not count as answers.
- Escalation, retirement and counts belong to authored Lisp; semantic grades and final recommendations remain open to the learner's and operator's judgment.
