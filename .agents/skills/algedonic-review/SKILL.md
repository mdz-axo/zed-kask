---
name: algedonic-review
core: true
description: "Human-in-the-loop review and triage of the algedonic alert backlog. Queries pending escalations, the algedonic event log, and system health, then synthesizes a structured triage briefing with per-alert severity, domain, and recommended action (resolve, dismiss, investigate, escalate-to-human). The operator reviews and acts on each alert, closing the feedback loop. Invoked when the algedonic log approaches its cap or on operator demand."
---

# Algedonic Review

Human-in-the-loop review and triage of the algedonic alert backlog. The algedonic system is the cybernetic regulation loop's pain/pleasure feedback — variety deficits, energy exhaustion, outcome plateaus, grounding violations. When these signals breach threshold, the cybernetics loop escalates alerts to a durable review queue. This skill reviews that queue, synthesizes a triage briefing, and guides the operator through resolving or dismissing each alert.

## When to Use

- The `AlgedonicLogApproachingCap` signal fired (the in-memory alert log is ≥80% full).
- The operator wants to review accumulated algedonic alerts and escalations.
- The operator wants to triage pending escalations before they accumulate further.
- The operator wants a structured digest of recent regulation events for operational awareness.


## When NOT to Use

- A `wiring-closed` or `broken` `loop_reading` — investigate the loop wiring before triage; alerts from a loop that never ticked are not trustworthy input.
- Autonomous resolution — the skill proposes and the operator decides; it never resolves or dismisses on its own.
- Clearing the in-memory log as the goal — `AlgedonicLogApproachingCap` is the trigger, not a condition this skill clears; the log self-evicts when full.

## Instructions

### SENSE — Query alert backlog (step 1)

1. Step 1 calls two curator MCP tools directly (no template): `curator_escalations` (pending backlog) and `curator_algedonic_log` (24h lookback).
2. `curator_status` is an agent tool, not an MCP tool — the skill cannot batch-call it from `mcp_batch`. However, the skill body instructs the agent to call `curator_status` separately (outside the batch) to retrieve the `loop_reading` field, which reports the trust/absence assembly verdict (wiring-closed / turning / broken / unobserved). This reading is critical context for triage: a `wiring-closed` reading means the regulation loop has never ticked — alerts may be stale or missing, and the operator should investigate the loop wiring before acting on individual alerts.
3. Keep the `curator_status` response as `status_result` (including `loop_reading`, `alert_log_count`, `alert_log_cap`, and `alert_log_approaching_cap`) and the two MCP responses as `escalations_result` and `algedonic_result`. A missing status or failed channel is unknown, not healthy or an empty backlog.
4. If `loop_reading` is `wiring-closed` or `broken`, stop before triage, report the structural concern and the raw status, and investigate the loop wiring. If the reading is missing or `unobserved`, surface the uncertainty rather than claiming a healthy loop.
5. The algedonic log's `reg.outcome.loop_quality` events carry `heartbeat: true` on the hourly idle emission (tick 1, then every 360 ticks). Absence of a heartbeat for more than two hourly intervals is a structural concern — a dead ticker and a converged loop are otherwise indistinguishable (both produce silence). Flag it alongside the `loop_reading` when the window was actually observed.

### TRIAGE — Synthesize triage briefing (step 2)

1. Only after the SENSE gate, render `algedonic-review/triage-briefing` with `escalations_result`, `algedonic_result`, and the complete `status_result`. A successful response with zero alerts is a no-alert review, not an error; a failed or unavailable response is not a no-alert review.
2. Each alert is classified from observed severity or valid deficit/threshold data (Critical → act now, Warning → act soon, Info → acknowledge). Missing or invalid severity remains `Unknown` and requires investigation; alert `confidence` is not a severity substitute.
3. Each alert gets a recommended action: `resolve` (issue addressed), `dismiss` (not actionable), `investigate` (needs root-cause analysis), or `escalate_to_human` (beyond curator authority).
4. The briefing includes the alert log cap status (count/cap, approaching flag) so the operator knows whether eviction is imminent.
5. The briefing carries the observed `loop_reading` and cap fields from `status_result` to `present-triage`; never fill unknown measurements with healthy defaults. A broken or wiring-closed reading was already stopped at SENSE.

### PRESENT — Render triage report (step 3)

1. Before rendering `algedonic-review/present-triage`, inspect the triage JSON: `escalation_triage` and `algedonic_digest` must be arrays and `summary` an object with numeric counts. A malformed shape is a failed handoff, not a no-alert result; stop and report it. Only then render the conversational summary with markdown tables.
2. The summary opens with the observed loop reading and alert log cap status (or explicitly unavailable status), then the escalation backlog table and algedonic event digest. A successful empty backlog is presented as no pending alerts.
3. Each escalation entry includes: ID, domain, severity, created_at, recommended action, and a one-line description.
4. The presentation closes with a prompt for the operator to act on each alert.

### ACT — Execute operator decisions (step 4)

1. Render the `algedonic-review/execute-decisions` template to produce a structured list of resolve/dismiss calls.
2. The operator reviews the briefing and specifies which alerts to resolve or dismiss.
3. For each confirmed decision with an observation-backed note applicable to that ID, the skill calls `curator_escalation_resolve` or `curator_escalation_dismiss` with the escalation ID and the note. The template produces planned calls only; execute them separately, retain each tool receipt, and do not report a planned call as completed.
4. The skill does NOT autonomously resolve or dismiss — the operator must confirm each decision.
5. Every resolve/dismiss verdict carries evidence: the resolution note must cite the observation that settles the alert (a reading taken, a log line, a metric re-checked). A verdict with nothing attached is a laundering UI pointed at the regulation loop — it costs the loop a correction, so it must cost the reviewer an observation. If the operator cannot name the evidence, the recommended action is `investigate`, not `resolve`.

### VERIFY — Confirm backlog cleared (step 5)

1. After executing any planned calls, collect their real success/error receipts and re-query `curator_escalations`. If the re-query fails, the remaining backlog is unknown; never infer clearance from a planned call or a missing response. Skip re-query when no calls were made.
2. Render `algedonic-review/verify-cleared` with `decisions` as an array of `{id, tool, success, response, error}` built from actual MCP responses (not the planned call list), plus the post-query response. Report confirmed resolutions, dismissals, failures and what remains pending. Without decisions, report no actions taken; do not imply the operator approved a call.
3. Never recommend clearing the in-memory log as part of this workflow; it self-evicts. If the backlog is unchanged, report that observation without claiming the review cleared it.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `triage-briefing.j2` | Structure the two signal channels (escalations + algedonic) into a triage list with per-alert severity and recommended action. |
| `present-triage.j2` | Render the triage briefing as a conversational summary with markdown tables, opening with the alert log cap status. |
| `execute-decisions.j2` | Produce the structured resolve/dismiss call list from the operator's decisions. |
| `verify-cleared.j2` | Summarize what was resolved, dismissed, and what remains pending. |

To render a template, call the `render_template` tool with the template ref (e.g., `algedonic-review/triage-briefing`) and a context object with the required variables.

## Constraints

- All templates run at `visibility: Public`.
- Human-in-the-loop: the skill proposes, the operator decides. The skill does NOT autonomously resolve or dismiss alerts.
- Ground every claim in the raw alert data. Do not fabricate alerts or severities not present in the inputs.
- A successful empty alert list means zero pending alerts. Failed, missing, and empty-success responses are different states; never silently substitute one for another.
- The in-memory algedonic log is a capped ring buffer (default 200 entries). The skill reviews the durable escalation queue, not the in-memory log — the in-memory log self-evicts when full. The `AlgedonicLogApproachingCap` signal is the trigger for running this skill, not a condition the skill itself clears.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.

## Design References

- Beer's Viable System Model — algedonic signals are the S1→S5 escalation path (System 1 pain → System 5 executive attention).
- Ashby's Law of Requisite Variety — the alert backlog is the variety the regulator could not absorb autonomously; human review is the external variety amplifier.
- Conant-Ashby theorem — "every good regulator of a system must be a model of that system." The triage briefing is the operator's model of the regulation system's state.
- Toyota Andon cord — algedonic alerts are the digital Andon; this skill is the structured response (not just acknowledgment).
