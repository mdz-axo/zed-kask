---
name: algedonic-review
core: true
description: "Human-in-the-loop algedonic review and skill gemba walk through the Algedonic review kanban board. The board is the worklist and durable review record; skill evaluation happens only in this review."
---

# Algedonic Review

The **Algedonic review** board in the kanban panel is the shared worklist for the operator and Curator. The regulation loop places new alerts in Backlog and records repeats as comments on the same open condition card. Skill-change proposals also enter this board, not a separate folder. The capped algedonic log is context, not the review backlog; it self-evicts and is never cleared merely to finish a review.

## SENSE — Read the worklist

1. Call `kanban_board_list` to find **Algedonic review**, then `kanban_task_list` for every column. If the board/tool cannot be read, report the backlog as unknown, not empty. Open the board in the kanban panel with the operator; the panel and its existing kanban widget are the visual artifact. `curator_status` supplies loop health, the awaiting-review count (all board cards not Done), and log-cap readings; `curator_algedonic_log` provides recent diagnostic events. Do not confuse latest-cycle new alerts with cards awaiting review.
2. If `loop_reading` is `wiring-closed` or `broken`, investigate the loop wiring before judging its alerts. If the reading is unavailable, state the uncertainty. A heartbeat absent for more than two observed hourly intervals is a structural concern, not proof of a healthy quiet loop.
3. Read each open card's description, comments, deliverables and verification criteria. An unreadable card is unknown. Backlog means arrived, Ready triaged, In Progress decided/delegated, Review fixed or self-recovered with evidence, Done verified.

## TRIAGE and ACT — Work the standard columns

4. Brief the operator using card IDs, observed severity, evidence and next action. Missing/invalid severity stays unknown; confidence is not a severity substitute. Record triage in a comment and move Backlog → Ready. Ask the operator for the actual decision; do not infer it.
5. For a decided item, record the decision and source observation on its card, assign it or use `kanban_task_spawn` / `spawn_agent` with clear instructions when work is delegated, and move Ready → In Progress. Attach outputs with `kanban_task_add_deliverable`. Do not create a second worklist, a dashboard or a dated review note.
6. After a fix or self-recovery, attach the fresh observation to that card and move to Review through the standard columns. Self-recovery is evidence for review, **not** authorization to mark Done. A 7-day post-advice observation is recorded as the verification evidence on this same card using `kanban_task_verify` only when the operator (or Curator under a recorded grant) actually judges the evidence; do not recreate an advice baseline/receipt/publication protocol in comments.
7. Done requires `kanban_task_verify` on a card in Review with the actual observation. The operator verifies by default. A Curator verification requires a grant recorded on that card with its scope; if verification is explicitly skipped under an authorized grant, record who skipped and why without pretending that an unverified change is verified. Re-query `kanban_task_list` after actions; tool-call intent is not a receipt and failed reads do not prove clearance.

## GEMBA WALK — Skill evaluation with the operator

8. Go to skill execution: call `reg_query` for `reg.skill` over the review window (default seven days), inspect `skill_use_issue:<skill>` memories for affected skills and look at prior `record_skill_feedback` entries. Activation means the skill loaded, not that its work was good. Classify tool failures by `failure_origin` before blaming a skill. Read skill-proposal cards and their linked evidence from the same Algedonic review board; a self-assessment on a card is a claim, not independent evidence.
9. Present observed outcomes, operator feedback, issues, missing evidence and each proposal card to the operator. **Skill evaluation happens only here**, separately from the executing session. Wait for the operator's actual verdict; record it via `record_skill_feedback` and on the proposal card with a cited observation. The operator holds proposal authority unless a scoped grant to the Curator is recorded on the card.
10. An accepted proposal or schema-evolution request is executed through an assigned or spawned agent from its card, with affected files, evidence and verification criteria recorded there. Move it to In Progress; attach the result, then move to Review. Verify through `kanban_task_verify` only after the operator or authorized Curator checks the predeclared work. An undecided or unverified card remains open for the next review.

## Constraints

- Do not autonomously evaluate a skill from the session that ran it, accept a proposal without authority, or claim that a planned call succeeded.
- Every verdict names the observation that could settle it. When that evidence is missing, investigate rather than mark Done.
- A failed status/board/log/memory read remains an explicit visibility gap. The algedonic log approaching its cap triggers review, not deletion of the log.
