---
name: algedonic-review
core: true
description: "Human-in-the-loop review and triage of the algedonic alert backlog. Queries pending escalations, the algedonic event log, and system health, then synthesizes a structured triage briefing with per-alert severity, domain, and recommended action (resolve, dismiss, investigate, escalate-to-human). The operator reviews and acts on each alert, closing the feedback loop. Invoked when the algedonic log approaches its cap or on operator demand."
steps:
  - id: sense
    tools:
      - curator_escalations
      - curator_algedonic_log
  - id: triage
    tools:
      - render_template
  - id: present
    tools:
      - render_template
  - id: execute
    tools:
      - curator_escalation_resolve
      - curator_escalation_dismiss
  - id: verify
    tools:
      - render_template
---

# Algedonic Review

Human-in-the-loop review and triage of the algedonic alert backlog. The algedonic system is the cybernetic regulation loop's pain/pleasure feedback — variety deficits, energy exhaustion, outcome plateaus, grounding violations. When these signals breach threshold, the cybernetics loop escalates alerts to a durable review queue. This skill reviews that queue, synthesizes a triage briefing, and guides the operator through resolving or dismissing each alert.

## When to Use

- The `AlgedonicLogApproachingCap` signal fired (the in-memory alert log is ≥80% full).
- The operator wants to review accumulated algedonic alerts and escalations.
- The operator wants to triage pending escalations before they accumulate further.
- The operator wants a structured digest of recent regulation events for operational awareness.
- The operator wants to clear reviewed alerts to free the in-memory log.

## Instructions

### SENSE — Query alert backlog (step 1)

1. Step 1 calls two curator MCP tools directly (no template): `curator_escalations` (pending backlog) and `curator_algedonic_log` (24h lookback).
2. `curator_status` is an agent tool, not an MCP tool — the skill cannot batch-call it from `mcp_batch`. However, the skill body instructs the agent to call `curator_status` separately (outside the batch) to retrieve the `loop_reading` field, which reports the trust/absence assembly verdict (wiring-closed / turning / broken / unobserved). This reading is critical context for triage: a `wiring-closed` reading means the regulation loop has never ticked — alerts may be stale or missing, and the operator should investigate the loop wiring before acting on individual alerts.
3. Results are keyed by `escalations` and `algedonic` in the result of step 1. The `curator_status` result (called separately) provides `loop_reading`.
4. The algedonic log's `reg.outcome.loop_quality` events carry `heartbeat: true` on the hourly idle emission (tick 1, then every 360 ticks). Absence of a heartbeat for more than two hourly intervals is a structural concern — a dead ticker and a converged loop are otherwise indistinguishable (both produce silence). Flag it in the briefing alongside the `loop_reading`.

### TRIAGE — Synthesize triage briefing (step 2)

1. Render the `algedonic-review/triage-briefing` template to structure the two signal channels (escalations + algedonic) into a triage list.
2. Each alert is classified by severity (Critical → act now, Warning → act soon, Info → acknowledge).
3. Each alert gets a recommended action: `resolve` (issue addressed), `dismiss` (not actionable), `investigate` (needs root-cause analysis), or `escalate_to_human` (beyond curator authority).
4. The briefing includes the alert log cap status (count/cap, approaching flag) so the operator knows whether eviction is imminent.
5. The briefing opens with the `loop_reading` from `curator_status`. If the reading is `wiring-closed` or `broken`, the briefing flags this as a structural concern that takes priority over individual alert triage — a loop that has never run cannot produce trustworthy alerts, and a broken loop may be generating false positives.

### PRESENT — Render triage report (step 3)

1. Render the `algedonic-review/present-triage` template as a conversational summary with markdown tables.
2. The summary opens with the alert log cap status, then the escalation backlog table, then the algedonic event digest.
3. Each escalation entry includes: ID, domain, severity, created_at, recommended action, and a one-line description.
4. The presentation closes with a prompt for the operator to act on each alert.

### ACT — Execute operator decisions (step 4)

1. Render the `algedonic-review/execute-decisions` template to produce a structured list of resolve/dismiss calls.
2. The operator reviews the briefing and specifies which alerts to resolve or dismiss.
3. For each decision, the skill calls `curator_escalation_resolve` or `curator_escalation_dismiss` with the escalation ID and a resolution note.
4. The skill does NOT autonomously resolve or dismiss — the operator must confirm each decision.
5. Every resolve/dismiss verdict carries evidence: the resolution note must cite the observation that settles the alert (a reading taken, a log line, a metric re-checked). A verdict with nothing attached is a laundering UI pointed at the regulation loop — it costs the loop a correction, so it must cost the reviewer an observation. If the operator cannot name the evidence, the recommended action is `investigate`, not `resolve`.

### VERIFY — Confirm backlog cleared (step 5)

1. Step 5 is an `execute` step (no template): re-queries `curator_escalations` to confirm the backlog is reduced after the operator's decisions. Skipped on the first pass (no decisions executed).
2. Step 6 renders the `algedonic-review/verify-cleared` template to summarize what was resolved, what was dismissed, and what remains pending.
3. The `verify-cleared` template also produces a recommendation for the agent to execute `curator_clear_algedonic_log` (an agent tool the skill cannot call from `mcp_batch`) to clear the in-memory log.
4. If the backlog is not reduced (operator declined to act), the skill notes this and exits — the alerts remain for the next review cycle.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `verify-cleared.j2` | Summarize what was resolved, dismissed, and what remains pending. |

To render a template, call the `render_template` tool with the template ref (e.g., `essentialist/essentialist-flow`) and a context object with the required variables.

## Constraints

- All templates run at `visibility: Public`.
- Human-in-the-loop: the skill proposes, the operator decides. The skill does NOT autonomously resolve or dismiss alerts.
- Ground every claim in the raw alert data. Do not fabricate alerts or severities not present in the inputs.
- If a signal channel returned an error or empty result, note it in the briefing — do not silently omit it.
- The in-memory algedonic log is a capped ring buffer (default 200 entries). The skill reviews the durable escalation queue, not the in-memory log — the in-memory log self-evicts when full. The `AlgedonicLogApproachingCap` signal is the trigger for running this skill, not a condition the skill itself clears.
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.

## Design References

- Beer's Viable System Model — algedonic signals are the S1→S5 escalation path (System 1 pain → System 5 executive attention).
- Ashby's Law of Requisite Variety — the alert backlog is the variety the regulator could not absorb autonomously; human review is the external variety amplifier.
- Conant-Ashby theorem — "every good regulator of a system must be a model of that system." The triage briefing is the operator's model of the regulation system's state.
- Toyota Andon cord — algedonic alerts are the digital Andon; this skill is the structured response (not just acknowledgment).
- `docs/reports/gemba-loop-specification.md` — the gemba loop's Observe → Decide → Act phases map to this skill's TRIAGE → PRESENT → ACT phases.
