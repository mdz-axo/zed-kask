---
name: grill-me
description: "Socratic interrogation skill. Tests deep understanding through escalating difficulty (Recall → Mechanism → Rationale → Edge Cases → Synthesis), of a human learner or of an AI agent's own work by a separate examiner. Probes gaps, challenges assumptions, produces gap analysis."
audience: user
---

# Grill Me

## Reference model

Paul & Elder, *The Art of Socratic Questioning* (2006), anchors questioning (`derived:socratic_method`). The five-level ladder is a local adaptation of Bloom (1956) / Krathwohl (2002, doi:10.1207/s15430421tip4104_2), not the revised taxonomy's six categories. Vygotsky (1978) anchors adaptive scaffolding (`derived:zone_of_proximal_development`). The 80%/40% thresholds, three-failure retirement, five-batch bound, two-regrade bound and the examiner-separation rule for an AI respondent are project rules, not published-model clauses. Carrier retrieval is not proof of primary-book inclusion or complete fidelity.

**Initial condition:** topic, respondent (`human` or `agent`), empty question bank and answer ledger, round/level 1, calibrate mode. **Target condition:** level-5 completion or batch 5, then an evidence-linked assessment; unanswered areas are Unassessed. On invalid data repair once and recheck, then report the gap. A repair/regrade never resets the bound.

## D/P labelling

Drafting, grading and recommendations are P, open to respondent/operator challenge. Appending answers, IDs, counts, feedback, the round and mode transitions, the regrade bound and evidence-reference checks are D. Use the authored jq filters below through `terminal` (`jq -ce` with JSON on stdin); jq is already a repository tool. No custom recursive helpers, event store or Rust controller. Keep the transcript in the conversation or name its deliberately retained artifact; no extra persistence service is required.

## When to Use

- Test a person's existing understanding, calibrate a baseline, or expose assumptions through progressively harder questions.
- Examine an AI agent's understanding of its own plan, estimate, finding or change before it is relied on — the interactive form of the self-challenge other skills ask for.

## When NOT to Use

- Teaching new material; use another method. Code review belongs to `code-review`; eliminating hypotheses by test belongs to `falsifiability`.

## The respondent

- **`human`** — a person answers in the conversation. Wait for each batch; a skip or hint is not an answer.
- **`agent`** — an AI agent answers about work it produced. Two rules apply, and the update filter enforces the second:
  1. **The examiner is not the respondent.** The session that produced the work does not draft the questions or grade the answers. Run the examiner as a separate session with `spawn_agent`, giving it this skill's name, the topic, the work under examination and the sources it rests on — not the producing session's reasoning. The producing session answers; the examiner asks, grades and assesses. Where no separate session can be spawned, one session plays both roles and the assessment must say `examiner: same_session` — a self-check, not an examination (the party being examined cannot be its own sole examiner).
  2. **Every answer cites evidence.** Each answer carries `evidence`: one or more locators the examiner can open — `path:line`, a tool result, a quoted source passage. An answer with no evidence is refused. Grade against the cited evidence, not against how fluent the answer is: an answer whose evidence does not show what it claims is a Gap, however plausible it reads.

## Instructions

1. Keep `question_bank: [{id,question,level,area}]`, one `answers: [{id,question_id,text,evidence,round,rating,failed,note}]` ledger and a `regrades_used` count. Never type an answer ID, a round number or a mode: **Feedback/update** appends answers, assigns their IDs and returns the next round and mode. Never join answers by position. `failed` means this answer has ever received Gap: later corrections change its latest rating, not its historical contribution (operator ruling 2026-10-20, proposal3568842d).
2. Render `grill-me/grill-me-round` in ask mode, execute drafting, and run **Question delivery** before showing the resulting records/explanations. Retain each newly assigned question once; retries reuse the original record. For a `human` respondent wait until the batch is answered or explicitly skipped. For an `agent` respondent send the admitted questions to the producing session and collect its answers with their evidence.
3. Append the actual answers by passing them as `new_answers` to **Feedback/update** with `answer_ids: []` and `evaluations: []`. It returns the ledger with the new rows and their `pending_ids`.
4. For grade mode, pass the requested `answer_ids`: normally the update's `pending_ids`, or existing IDs explicitly requested for regrading. The renderer selects rows from `feedback.answers`; do not build another answer-record copy. Execute grading. Pass those `answer_ids` and returned `evaluations` to **Feedback/update** with `new_answers: []`. A failed command admits nothing; otherwise replace the ledger with its returned `answers`, not an appended grading history. A regrade — grading an answer that already has a rating — is allowed at most twice per session; the filter counts them in `regrades_used` and refuses a third.
5. Use the returned `direction`, `next_level`, `next_round` and `next_mode` before asking again; carry `regrades_used` forward. Calibration requires actual graded answers and a proposed level 1–5; it seeds the first interrogate level without an extra escalation. Preserve that initial `calibrated_level` for final comparison. `await_grades` → grade; `stop: true` → assess; otherwise ask the next batch at the returned level, round and mode — at most five batches including calibration. The two never coincide: `stop` is false while any answer in the batch is ungraded, so batch 5 is graded before it is assessed. Regrades alone neither advance the batch nor ask questions. Zero answers holds, but batch 5 still stops.
6. Run **Assessment skeleton** on the exact bank/ledger, then render `grill-me/grill-me-assess` with its result as `areas`, the ledger and the initial calibrated level. Execute synthesis, then run **Final evidence** on its summary. This checks references/coverage, not semantic wisdom. Deliver the assessment and prioritized recommendations, with unresolved limitations visible; for an `agent` respondent state whether the examiner was a separate session.

Copyable round context (JSON-encode the outer `render_template.context`; keep inner arrays structured):
```json
{"topic":"<topic>","respondent":"human","phase":"ask","mode":"calibrate","current_level":1,"round_number":1,"question_bank":[],"answer_ids":[],"feedback":{"answers":[],"direction":"hold","retired_ids":[],"stop":false}}
```
For grade, change phase and pass the exact selected IDs with actual feedback. For subsequent ask, use that feedback and the returned level, round and mode with `answer_ids:[]`. Do not maintain a second running assessment. Final context:
```json
{"topic":"<topic>","respondent":"human","areas":[],"question_bank":[],"answers":[],"calibrated_level":null}
```

### Feedback/update

Input shape (substitute current records). One call does one thing: append (`new_answers` non-empty), grade (`answer_ids` and `evaluations` non-empty), or query (all three empty).
```json
{"question_bank":[],"answers":[],"new_answers":[],"answer_ids":[],"evaluations":[],"level":1,"round":1,"mode":"calibrate","calibrated_level":null,"respondent":"human","regrades_used":0}
```
Each `new_answers` entry is `{"question_id":<bank id>,"text":"<the answer>","evidence":["<locator>"]}`; `evidence` may be `[]` for a `human` respondent. Execute this filter with `jq -ce` on that JSON. Only its output replaces state. It rejects missing/duplicate/unknown grade identities, a second answer to one question in the same round, an `agent` answer without evidence, and a third regrade; current grades are overwritten in place while `failed` can only become true.
```jq
. as $s | [.question_bank[].id] as $qids | [.answers[].id] as $ids
| [.evaluations[].answer_id] as $gids | (.regrades_used // 0) as $regrades_before
| if (([.level,.round] | all(. >= 1 and . <= 5 and floor == .))
  and (.mode == "calibrate" or .mode == "interrogate")
  and (.respondent == "human" or .respondent == "agent")
  and ($qids|unique|length) == ($qids|length) and ($ids|unique|length) == ($ids|length)
  and ($gids|unique|length) == ($gids|length) and ($gids|sort) == (.answer_ids|sort)
  and all(($qids+$ids+$gids)[]; type == "number" and floor == . and . > 0)
  and all(.answers[]; . as $a | ($qids|index($a.question_id)) != null
    and (.text|type) == "string" and (.text|length) > 0
    and .round >= 1 and .round <= $s.round and (.round|floor) == .round
    and (.evidence|type) == "array" and all(.evidence[]; type == "string" and length > 0)
    and (.failed|type) == "boolean" and ([null,"Solid","Partial","Gap"]|index($a.rating)) != null
    and (.rating != "Gap" or .failed))
  and (.new_answers|type) == "array"
  and ((.new_answers|length) == 0 or (.answer_ids|length) == 0)
  and all(.new_answers[]; . as $n | ($qids|index($n.question_id)) != null
    and (.text|type) == "string" and (.text|length) > 0
    and (.evidence|type) == "array" and all(.evidence[]; type == "string" and length > 0)
    and ($s.respondent == "human" or (.evidence|length) > 0))
  and ([.new_answers[].question_id]|unique|length) == (.new_answers|length)
  and all(.new_answers[]; . as $n | all($s.answers[]; .question_id != $n.question_id or .round != $s.round))
  and all(.evaluations[]; . as $g | ($ids|index($g.answer_id)) != null
    and (["Solid","Partial","Gap"]|index($g.rating)) != null and (.note|type) == "string")
  and ((.answer_ids|length) == 0 or (if .mode == "calibrate" then
    .calibrated_level >= 1 and .calibrated_level <= 5 and (.calibrated_level|floor) == .calibrated_level
    else .calibrated_level == null end))) then . else error("invalid session or grade batch") end
| ([.answers[]|select(.rating != null)|.id]) as $graded
| ([.answer_ids[]|select(. as $id | ($graded|index($id)) != null)]|length) as $regrading
| ($regrades_before + (if $regrading > 0 then 1 else 0 end)) as $regrades
| if $regrades > 2 then error("regrade bound exceeded") else . end
| (($ids|max // 0)) as $last
| (.answers + (.new_answers|to_entries|map({id:($last+.key+1), question_id:.value.question_id,
    text:.value.text, evidence:.value.evidence, round:$s.round, rating:null, failed:false, note:null}))) as $appended
| INDEX(.evaluations[]; .answer_id|tostring) as $grades
| ($appended | map($grades[(.id|tostring)] as $g | if $g == null then . else
    . + {rating:$g.rating, note:$g.note, failed:(.failed or $g.rating == "Gap")} end)) as $rows
| [.question_bank[] | .id as $q | select(([$rows[]|select(.question_id == $q and .failed)]|length) >= 3) | .id] as $retired
| [$rows[]|select(.round == $s.round)] as $batch
| [$batch[]|select(.rating == null)|.id] as $pending
| ($batch|length) as $n | ([$batch[]|select(.rating == "Solid")]|length) as $solid
| (if ($pending|length) > 0 then "await_grades" elif $n == 0 then "hold"
   elif .round >= 5 then "complete" elif .mode == "calibrate" and (.answer_ids|length) > 0 then "hold"
   elif $solid*5 >= $n*4 then (if .level == 5 then "complete" else "escalate" end)
   elif $solid*5 >= $n*2 then "hold" else "reprobe" end) as $direction
| (($pending|length) == 0 and (.round >= 5 or $direction == "complete")) as $stop
| ($direction != "await_grades" and $n > 0 and ($stop|not)) as $advance
| {answers:$rows, retired_ids:$retired, pending_ids:$pending, direction:$direction,
   next_level:(if .mode == "calibrate" and (.answer_ids|length) > 0 then .calibrated_level
     elif $direction == "escalate" then .level+1 else .level end),
   next_round:(if $advance then .round+1 else .round end),
   next_mode:(if $advance then "interrogate" else .mode end),
   regrades_used:$regrades, stop:$stop}
```

### Question delivery

Input: `{"drafts":[],"explanations":[],"question_bank":[],"retired_ids":[],"level":1}`. Bind drafts/explanations from the ask output; only when the retired set is empty may the caller supply explanations `[]` directly. New IDs follow the bank's maximum; a retry uses its original ID/text. Semantic paraphrase detection and explanation correctness remain P.
```jq
. as $s | INDEX(.question_bank[]; .id|tostring) as $bank
| ((.question_bank|map(.id)|max // 0)+1) as $next
| if ((.drafts|length) >= 2 and (.drafts|length) <= 3
  and (.explanations|map(.question_id)|sort) == (.retired_ids|sort)
  and all(.explanations[]; (.explanation|type) == "string" and (.explanation|length) > 0))
  then .drafts|to_entries|map(.key as $slot | .value |
    if .retry_id == null then
      if (.question|type) != "string" or (.question|length) == 0 or (.area|type) != "string" or (.area|length) == 0
        or (.question as $text | any($s.question_bank[]; .question == $text)) then error("invalid new question")
      else {id:($next+$slot),question:.question,area:.area,level:$s.level} end
    else $bank[(.retry_id|tostring)] as $q |
      if $q == null or ($s.retired_ids|index($q.id)) != null or $q.level != $s.level
        or .question != null or .area != null then error("invalid retry") else $q end end)
  else error("invalid question count or retired explanations") end
| if (map(.id)|unique|length) == length and (map(.question)|unique|length) == length
  then . else error("duplicate question") end
```

### Assessment skeleton

Input: `{"question_bank":[],"answers":[]}` using the exact latest ledger. Returns one entry per asked area with its questions, its graded answer IDs and their latest ratings; `assessable: false` marks an area with no graded answer. Run with `jq -ce` and pass the result to the assess render as `areas`.
```jq
. as $s | INDEX(.question_bank[]; .id|tostring) as $bank
| [.question_bank[].area] | unique
| map(. as $area | {area:$area,
    question_ids:[$s.question_bank[]|select(.area == $area)|.id],
    graded_answer_ids:[$s.answers[]|select(.rating != null and $bank[(.question_id|tostring)].area == $area)|.id],
    latest_ratings:[$s.answers[]|select(.rating != null and $bank[(.question_id|tostring)].area == $area)|.rating]}
  | . + {assessable:((.graded_answer_ids|length) > 0)})
```

### Final evidence

Input: `{"question_bank":[],"answers":[],"summary":[]}` using the exact latest ledger and generated summary. Each asked area occurs once; graded summaries cite actual graded answers in that area, while areas without such evidence are Unassessed. Run with `jq -ce`; false/error blocks delivery.
```jq
. as $s | INDEX(.question_bank[]; .id|tostring) as $bank
| ((.summary|map(.area)|sort) == (.question_bank|map(.area)|unique|sort))
and all(.summary[]; . as $r |
  [$s.answers[]|select(.rating != null and $bank[(.question_id|tostring)].area == $r.area)|.id] as $eligible |
  if .rating == "Unassessed" then .answer_ids == [] and $eligible == []
  else (["Solid","Partial","Gap"]|index($r.rating)) != null and (.answer_ids|length) > 0
    and (.answer_ids|unique|length) == (.answer_ids|length)
    and all(.answer_ids[]; . as $id | ($eligible|index($id)) != null) end)
```

## Registry Templates

- `grill-me-round.j2`: ask or grade, with feedback between executions; no counters or copied assessments. For an `agent` respondent the grade prompt shows each answer's evidence and grades against it.
- `grill-me-assess.j2`: final evidence-linked area judgments and recommendations over the computed area skeleton.

## Regression case

The native renderer tests run these exact authored jq filters, not translated formulas: `bash kask/scripts/cargo-test-nonzero.sh -p agent --lib grill_`. Cover appended answers and their assigned IDs, regrades versus distinct failures, the two-regrade bound, latest-grade changes, threshold/batch boundaries and the returned round and mode, bad/missing IDs, an `agent` answer without evidence, legal/retired retries, explanation coverage, the area skeleton and Unassessed evidence. These are computation/handoff checks, not a study of whether examination improves a learner or an agent.

## Constraints

- A regrade never creates an answer or resets its failed flag, ID or batch count; at most two regrades per session.
- Grade before asking again; deliver only admitted questions/explanations and evidence-linked final ratings.
- An `agent` respondent's answer without evidence is not an answer. A same-session examination is reported as a self-check.
- Keep human judgment over semantic grades and recommendations. Do not recreate removed event histories, shadow summaries or general-purpose validators.
