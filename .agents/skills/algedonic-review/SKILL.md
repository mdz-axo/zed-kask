---
name: algedonic-review
core: true
description: "Human-in-the-loop algedonic review and skill gemba walk through the Algedonic review kanban board. The board is the worklist and durable review record; skill evaluation happens only in this review."
audience: curator
---

# Algedonic Review

The **Algedonic review** board in the kanban panel is the shared worklist for the operator and Curator. The regulation loop places new alerts in Backlog and records changed repeat observations or hourly unchanged-condition checkpoints as comments on the same open card; identical ten-second ticks do not each add a comment. Skill-change proposals also enter this board, not a separate folder. The capped algedonic log is context, not the review backlog; it self-evicts and is never cleared merely to finish a review.

## Reference model, conditions and labels

Beer, *Brain of the Firm* (1972) — the algedonic channel (`onto_anchor` → derived `algedonic_signal`, operator ruling 2026-09-25): an alarm from System 1 to System 5 that carries only the signal that something breached; the policy level (the operator) decides. Rother, *Toyota Kata* (2010) for the gemba walk (`onto_anchor` → derived `gemba_walk`).

- **Initial condition:** the board's columns as read by `kanban_task_list`, the `curator_status` `loop_reading` field, and the `reg.skill` window — or an explicit visibility gap where a read failed.
- **Target condition:** every card in Review carries the observation that settles it, every open alert names its next action, and every proposal card has an operator verdict recorded on the card.
- **D/P labelling:** board reads, column moves, `kanban_task_verify` receipts, the Step 2 (`ALG-1`) SENSE-health gate over `curator_status`'s `loop_reading` and the Step 10 (`ALG-2`) closure gate over the `kanban_task_list` rows are D (the tools' outputs are the oracle; both gates read them as returned); severity triage, the fix decision and the skill verdict are P — the operator's, never this session's. The one bounded loop: steps 1–10 run once per review; a card whose settling observation is missing re-enters at step 3 for that card only, and the review ends when the target condition holds or the remaining gaps are named on their cards.

## SENSE — Read the worklist

1. Call `kanban_board_list` to find **Algedonic review**, then `kanban_task_list` for every column. If the board/tool cannot be read, report the backlog as unknown, not empty. Open the board in the kanban panel with the operator; the panel and its existing kanban widget are the visual artifact. `curator_status` supplies loop health, the awaiting-review count (all board cards not Done), and log-cap readings; `curator_algedonic_log` provides recent diagnostic events. Do not confuse latest-cycle new alerts with cards awaiting review.
2. Gate SENSE health via `lisp_eval` (`ALG-1`) over the `curator_status` field as returned:
   - form: `(cond ((not (stringp loop_reading)) "unobserved") ((member loop_reading (list "wiring-closed" "broken")) "investigate_wiring") ((string= loop_reading "unobserved") "unobserved") (t "proceed"))`
   - env: `{"loop_reading": <curator_status.loop_reading, verbatim — null when the field is absent>}`
   - `investigate_wiring` (`wiring-closed` or `broken`): investigate the loop wiring before judging its alerts. `unobserved` (the reading is `unobserved` or absent): state the uncertainty; a quiet loop that cannot be observed is not a healthy one. `proceed` otherwise. A board that could not be read is reported as unknown at step 1; it is not an input here.
3. Read each open card's description, comments, deliverables and verification criteria. An unreadable card is unknown. Backlog means arrived, Ready triaged, In Progress decided/delegated, Review fixed or self-recovered with evidence, Done verified.

## TRIAGE and ACT — Work the standard columns

4. Brief the operator using card IDs, observed severity, evidence and next action. Missing/invalid severity stays unknown; confidence is not a severity substitute. Record triage in a comment and move Backlog → Ready. Ask the operator for the actual decision; do not infer it.
5. For a decided item, record the decision and source observation on its card, assign it or use `kanban_task_spawn` / `spawn_agent` with clear instructions when work is delegated, and move Ready → In Progress. Attach outputs with `kanban_task_add_deliverable`. Do not create a second worklist, a dashboard or a dated review note.
6. After a fix or self-recovery, attach the fresh observation to that card and move to Review through the standard columns. Self-recovery is evidence for review, **not** authorization to mark Done. If the condition worsens again before verification, the connector comments on the same card and moves it from Review back to In Progress; investigate the fresh observation before verifying. A 7-day post-advice observation is recorded as the verification evidence on this same card using `kanban_task_verify` only when the operator (or Curator under a recorded grant) actually judges the evidence; do not recreate an advice baseline/receipt/publication protocol in comments.
7. Done requires `kanban_task_verify` on a card in Review with the actual observation. The operator verifies by default. A Curator verification requires a grant recorded on that card with its scope; if verification is explicitly skipped under an authorized grant, record who skipped and why without pretending that an unverified change is verified. **Operator standing grant (2026-11-02, recorded in-walk):** for the regulation loop's own alert cards — escalations and threshold trips — whose settling observation is recorded on the card, the Curator verifies to Done without re-asking; the grant's scope is exactly that card class. Skill-proposal cards are outside this standing grant: they change agent behavior, and their acceptance stays with the operator under the designer-cannot-judge rule. Re-query `kanban_task_list` after actions; tool-call intent is not a receipt and failed reads do not prove clearance. The board itself enforces the receipt: its Done column is verification-gated (`done_requires_verification`), so `kanban_task_move` to Done is refused and only `kanban_task_verify` with non-empty evidence reaches it — a card in Done has a verify receipt by construction. The grant and the self-recovery rule are the operator's and this session's discipline, recorded on the card; no tool output carries them, so no gate claims to count them.

## GEMBA WALK — Skill evaluation with the operator

8. Go to skill execution: call `reg_query` for `reg.skill` over the review window (default seven days), inspect `skill_use_issue:<skill>` memories for affected skills and look at prior `record_skill_feedback` entries. Activation means the skill loaded, not that its work was good. Classify tool failures by `failure_origin` before blaming a skill. Read skill-proposal cards and their linked evidence from the same Algedonic review board; a self-assessment on a card is a claim, not independent evidence.
9. Present observed outcomes, operator feedback, issues, missing evidence and each proposal card to the operator. **Skill evaluation happens only here**, separately from the executing session. Wait for the operator's actual verdict; record it via `record_skill_feedback` and on the proposal card with a cited observation. Batch the asks: one briefing for the whole walk, with a recommended disposition per card, so the operator can answer in a word — and under the standing grant (step 7), settled alert cards need no ask at all. Observed 2026-11-02: a fourteen-item per-card ask drew "this seems like stuff that should be automated"; the batched-plus-standing-grant form is the accepted correction. The operator holds proposal authority unless a scoped grant to the Curator is recorded on the card. As a standing question, sample a few decisions from the review window against the values charter's discernment test — does this make human life more human? (`kask/docs/architecture/core/magnifica-humanitas.md`) — and present any misfit as its own observation.
10. An accepted proposal or schema-evolution request is executed through an assigned or spawned agent from its card, with affected files, evidence and verification criteria recorded there. Move it to In Progress; attach the result, then move to Review. Verify through `kanban_task_verify` only after the operator or authorized Curator checks the predeclared work. An undecided or unverified card remains open for the next review. Before closing the review session, re-read every column with `kanban_task_list` and check the target condition via `lisp_eval` (`ALG-2`) over the rows as returned:
    - form: `(begin (define text-of (lambda (row key) (if (stringp (assoc key row)) (assoc key row) ""))) (define silent (lambda (rows) (if (is_null rows) (list) (if (is_null (assoc "activity" (car rows))) (cons (text-of (car rows) "task_id") (silent (cdr rows))) (silent (cdr rows)))))) (define in (lambda (status rows) (if (is_null rows) (list) (if (string= (text-of (car rows) "status") status) (cons (car rows) (in status (cdr rows))) (in status (cdr rows)))))) (define rows (if (listp tasks) tasks (list))) (define unread (not (or (eq tasks (list)) (and (listp tasks) (not (is_null tasks)))))) (define review-gap (silent (in "review" rows))) (define open-gap (append (silent (in "backlog" rows)) (silent (in "ready" rows)) (silent (in "in_progress" rows)))) (list (list "verdict" (cond (unread "board_unread") ((and (is_null review-gap) (is_null open-gap)) "target_met") (t "gaps_remain"))) (list "review_without_observation" review-gap) (list "open_without_next_action" open-gap) (list "cards_read" (length rows))))`
    - env: `{"tasks": <the concatenated kanban_task_list rows of all five columns, verbatim; null when any column read failed>}`
    - receipts (beside the form; `check-skill-lisp-forms` evaluates each line's env over the form above and compares the expected result exactly; the observed cards' activity carries kind "comment"):
     ```lisp-receipts
     {"gate": "ALG-2", "case": "mixed-board-names-review-and-open-gaps", "env": {"tasks": [{"task_id":"a","board_id":"b","title":"a","status":"review","assignee":null,"criteria":[],"advances_count":0,"activity":{"text":"observed: settled","kind":"comment","at":"2026-11-12T10:00:00Z"}},{"task_id":"b","board_id":"b","title":"b","status":"review","assignee":null,"criteria":[],"advances_count":0,"activity":null},{"task_id":"c","board_id":"b","title":"c","status":"backlog","assignee":null,"criteria":[],"advances_count":0,"activity":null},{"task_id":"d","board_id":"b","title":"d","status":"in_progress","assignee":null,"criteria":[],"advances_count":0,"activity":{"text":"observed: settled","kind":"comment","at":"2026-11-12T10:00:00Z"}},{"task_id":"e","board_id":"b","title":"e","status":"done","assignee":null,"criteria":[],"advances_count":0,"activity":null}]}, "expected": {"verdict":"gaps_remain","review_without_observation":["b"],"open_without_next_action":["c"],"cards_read":5}}
     {"gate": "ALG-2", "case": "ready-without-observation-is-an-open-gap", "env": {"tasks": [{"task_id":"r","board_id":"b","title":"r","status":"ready","assignee":null,"criteria":[],"advances_count":0,"activity":null}]}, "expected": {"verdict":"gaps_remain","review_without_observation":[],"open_without_next_action":["r"],"cards_read":1}}
     {"gate": "ALG-2", "case": "observed-review-and-open-in-progress-is-target-met", "env": {"tasks": [{"task_id":"a","board_id":"b","title":"a","status":"review","assignee":null,"criteria":[],"advances_count":0,"activity":{"text":"observed: settled","kind":"comment","at":"2026-11-12T10:00:00Z"}},{"task_id":"d","board_id":"b","title":"d","status":"in_progress","assignee":null,"criteria":[],"advances_count":0,"activity":{"text":"observed: settled","kind":"comment","at":"2026-11-12T10:00:00Z"}}]}, "expected": {"verdict":"target_met","review_without_observation":[],"open_without_next_action":[],"cards_read":2}}
     {"gate": "ALG-2", "case": "empty-board-is-target-met", "env": {"tasks": []}, "expected": {"verdict":"target_met","review_without_observation":[],"open_without_next_action":[],"cards_read":0}}
     {"gate": "ALG-2", "case": "failed-read-is-board-unread", "env": {"tasks": null}, "expected": {"verdict":"board_unread","review_without_observation":[],"open_without_next_action":[],"cards_read":0}}
     {"gate": "ALG-2", "case": "textual-error-is-board-unread", "env": {"tasks": "board not found"}, "expected": {"verdict":"board_unread","review_without_observation":[],"open_without_next_action":[],"cards_read":0}}
     ```
    - `board_unread`: a read failed — report the backlog as unknown, not clear. `gaps_remain`: `review_without_observation` lists the Review cards with no activity at all (no comment carries a settling observation), `open_without_next_action` the Backlog/Ready/In Progress cards with none (no triage or decision was recorded) — return to step 3 for each named card. `target_met`: every open card carries a recorded comment; whether that comment is the right observation or verdict is the operator's reading, not the gate's.

## Regression case

Receipt executed live (2026-10-01, batch-14 audit), read-only:

- SENSE phase board read: `kanban_board_list` returns the **Algedonic
  review** board (5 columns: Backlog / Ready / In Progress / Review /
  Done) — the shared worklist the skill documents, read without
  mutating it. A failed read would be reported as a visibility gap,
  never as an empty backlog.

The skill's oracle is the kanban tool receipts themselves (board reads,
column moves, `kanban_task_verify`) plus the two gates, which
`bash kask/scripts/check-skill-lisp-forms.sh` evaluates over the authored
forms (`scientific_gate_alg_`): `ALG-1` over `homeostatic` and `turning`
(`proceed`), `broken` and `wiring-closed` (`investigate_wiring`),
`unobserved` and an absent field (`unobserved`); `ALG-2` over a board
with a commented and an uncommented Review card, an uncommented Backlog
card, a commented In Progress card and an uncommented Done card
(`gaps_remain`, naming exactly the two uncommented open cards), an
all-commented board (`target_met`), an empty board (`target_met`), and a
failed read passed as null or as an error string (`board_unread`). The
review never mutates the board merely to produce a receipt.

## Registry Templates

This skill owns no registry templates — the worklist is the Algedonic
review board and the kanban/curator tools.

## Constraints

- A failed status/board/log/memory read remains an explicit visibility gap. The algedonic log approaching its cap triggers review, not deletion of the log.
- A memory-maintenance finding proposed from this review names the memory-architecture valve it replicates or adjusts (decay setting, distillation cadence, forgetting horizon, manual prune valve, alarm sensors) and reconciles the store's state against that valve — routine architecture behavior is not a finding (operator correction 2026-10-13).
