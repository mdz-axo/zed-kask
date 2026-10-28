---
name: grill-me
description: "Socratic interrogation skill. Tests deep understanding through escalating difficulty (Recall → Mechanism → Rationale → Edge Cases → Synthesis). Probes gaps, challenges assumptions, produces gap analysis."
---

# Grill Me

## Reference model

Paul & Elder, *The Art of Socratic Questioning* (2006), anchors questioning (`derived:socratic_method`). The five-level ladder is a local adaptation of Bloom (1956) / Krathwohl (2002, doi:10.1207/s15430421tip4104_2), not the revised taxonomy's six categories. Vygotsky (1978) anchors adaptive scaffolding (`derived:zone_of_proximal_development`). The 80%/40% thresholds, three-failure retirement and five-batch bound are project rules, not published-model clauses. Carrier retrieval is not proof of primary-book inclusion or complete fidelity.

**Initial condition:** topic, empty question bank and answer ledger, round/level 1, calibrate mode. **Target condition:** level-5 completion or batch 5, then an evidence-linked assessment; unanswered areas are Unassessed. On invalid data repair once and recheck, then report the gap. A repair/regrade never resets the bound.

## D/P labelling

Drafting, grading and recommendations are P, open to learner/operator challenge. Ledger updates, counts, feedback, IDs and evidence-reference checks are D. Use the authored jq filters below through `terminal` (`jq -ce` with JSON on stdin); jq is already a repository tool. No custom recursive helpers, event store or Rust controller. Keep the transcript in the conversation or name its deliberately retained artifact; no extra persistence service is required.

## When to Use

- Test existing understanding, calibrate a baseline, or expose assumptions through progressively harder questions.

## When NOT to Use

- Teaching new material; use another method. Code review belongs to `code-review`; self-adversarial reasoning to `falsifiability`.

## Instructions

1. Keep `question_bank: [{id,question,level,area}]` and one `answers: [{id,question_id,text,round,rating,failed,note}]` ledger. Append actual learner answers with `rating:null, failed:false, note:null`; assign each distinct submission the next answer ID (`length + 1` deterministically). Regrading reuses that ID. Never join answers by position. `failed` means this answer has ever received Gap: later corrections change its latest rating, not its historical contribution (operator ruling 2026-10-20, proposal3568842d).
2. Render `grill-me/grill-me-round` in ask mode, execute drafting, and run **Question delivery** before showing the resulting records/explanations. Retain each newly assigned question once; retries reuse the original record. Wait until the batch is answered or explicitly skipped. A skip/hint is not an answer.
3. For grade mode, pass the requested `answer_ids`: normally the update's `pending_ids`, or existing IDs explicitly requested for regrading. The renderer selects rows from `feedback.answers`; do not build another answer-record copy. Execute grading. Pass those `answer_ids` and returned `evaluations` to **Feedback/update**. A failed command admits nothing; otherwise replace the ledger with its returned `answers`, not an appended grading history.
4. Use returned level/direction before asking again. Calibration requires actual graded answers and a proposed level 1–5; it seeds the first interrogate level without an extra escalation. Preserve that initial `calibrated_level` for final comparison. Then use interrogate mode. Complete/stop → assess; await_grades → grade; otherwise advance the batch, at most five including calibration. Regrades alone neither advance the batch nor ask questions. Zero answers holds, but batch 5 still stops.
5. Render `grill-me/grill-me-assess` on the exact bank/ledger and initial calibrated level. Execute synthesis, then run **Final evidence** on its summary. This checks references/coverage, not semantic wisdom. Deliver the assessment and prioritized recommendations, with unresolved limitations visible.

Copyable round context (JSON-encode the outer `render_template.context`; keep inner arrays structured):
```json
{"topic":"<topic>","phase":"ask","mode":"calibrate","current_level":1,"round_number":1,"question_bank":[],"answer_ids":[],"feedback":{"answers":[],"direction":"hold","retired_ids":[],"stop":false}}
```
For grade, change phase and pass the exact selected IDs with actual feedback. For subsequent ask, use that feedback and its level with `answer_ids:[]`. Do not maintain a second running assessment. Final context:
```json
{"topic":"<topic>","question_bank":[],"answers":[],"calibrated_level":null}
```

### Feedback/update

Input shape (substitute current records; empty IDs/evaluations queries without regrading):
```json
{"question_bank":[],"answers":[],"answer_ids":[],"evaluations":[],"level":1,"round":1,"mode":"calibrate","calibrated_level":null}
```
Execute this filter with `jq -ce` on that JSON. Only its output replaces state. It rejects missing/duplicate/unknown grade identities; current grades are overwritten in place while `failed` can only become true.
```jq
. as $s | [.question_bank[].id] as $qids | [.answers[].id] as $ids
| [.evaluations[].answer_id] as $gids
| if (([.level,.round] | all(. >= 1 and . <= 5 and floor == .))
  and (.mode == "calibrate" or .mode == "interrogate")
  and ($qids|unique|length) == ($qids|length) and ($ids|unique|length) == ($ids|length)
  and ($gids|unique|length) == ($gids|length) and ($gids|sort) == (.answer_ids|sort)
  and all(($qids+$ids+$gids)[]; type == "number" and floor == . and . > 0)
  and all(.answers[]; . as $a | ($qids|index($a.question_id)) != null
    and (.text|type) == "string" and (.text|length) > 0
    and .round >= 1 and .round <= $s.round and (.round|floor) == .round
    and (.failed|type) == "boolean" and ([null,"Solid","Partial","Gap"]|index($a.rating)) != null
    and (.rating != "Gap" or .failed))
  and all(.evaluations[]; . as $g | ($ids|index($g.answer_id)) != null
    and (["Solid","Partial","Gap"]|index($g.rating)) != null and (.note|type) == "string")
  and ((.answer_ids|length) == 0 or (if .mode == "calibrate" then
    .calibrated_level >= 1 and .calibrated_level <= 5 and (.calibrated_level|floor) == .calibrated_level
    else .calibrated_level == null end))) then . else error("invalid session or grade batch") end
| INDEX(.evaluations[]; .answer_id|tostring) as $grades
| (.answers | map($grades[(.id|tostring)] as $g | if $g == null then . else
    . + {rating:$g.rating, note:$g.note, failed:(.failed or $g.rating == "Gap")} end)) as $rows
| (.question_bank | map(.id as $q | [$q, ([$rows[]|select(.question_id == $q and .failed)]|length)])) as $counts
| [$rows[]|select(.round == $s.round)] as $batch
| [$batch[]|select(.rating == null)|.id] as $pending
| ($batch|length) as $n | ([$batch[]|select(.rating == "Solid")]|length) as $solid
| (if ($pending|length) > 0 then "await_grades" elif $n == 0 then "hold"
   elif .round >= 5 then "complete" elif .mode == "calibrate" and (.answer_ids|length) > 0 then "hold"
   elif $solid*5 >= $n*4 then (if .level == 5 then "complete" else "escalate" end)
   elif $solid*5 >= $n*2 then "hold" else "reprobe" end) as $direction
| {answers:$rows, attempt_counts:$counts, retired_ids:[$counts[]|select(.[1] >= 3)|.[0]],
   pending_ids:$pending, answered_count:$n, solid_count:$solid, direction:$direction,
   next_level:(if .mode == "calibrate" and (.answer_ids|length) > 0 then .calibrated_level
     elif $direction == "escalate" then .level+1 else .level end),
   stop:(.round >= 5 or $direction == "complete")}
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

- `grill-me-round.j2`: ask or grade, with feedback between executions; no counters or copied assessments.
- `grill-me-assess.j2`: final evidence-linked area judgments and recommendations.

## Regression case

The native renderer tests run these exact authored jq filters, not translated formulas: `bash kask/scripts/cargo-test-nonzero.sh -p agent --lib grill_`. Cover regrades versus distinct failures, latest-grade changes, threshold/batch boundaries, bad/missing IDs, legal/retired retries, explanation coverage and Unassessed evidence. These are computation/handoff checks, not a learner-effectiveness study.

## Constraints

- A regrade never creates an answer or resets its failed flag, ID or batch count.
- Grade before asking again; deliver only admitted questions/explanations and evidence-linked final ratings.
- Keep human judgment over semantic grades and recommendations. Do not recreate removed event histories, shadow summaries or general-purpose validators.
