---
name: algedonic-review
core: true
visibility: public
description: "Human-in-the-loop review and triage of the algedonic alert backlog. Queries pending escalations, the algedonic event log, and system health, then synthesizes a structured triage briefing with per-alert severity, domain, and recommended action (resolve, dismiss, investigate, escalate-to-human). The operator reviews and acts on each alert, closing the feedback loop. Invoked when the algedonic log approaches its cap or on operator demand."
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

1. Call `curator_escalations` to get the pending escalation backlog.
2. Call `curator_algedonic_log` with a 24-hour lookback to get recent algedonic events.
3. Call `curator_status` to get the current system health (alert log count, approaching cap, critical alerts).
4. All three calls are independent and run concurrently via `mcp_batch`.

### TRIAGE — Synthesize triage briefing (step 2)

1. Render the `algedonic-review/triage-briefing` template to structure the alerts into a triage list.
2. Each alert is classified by severity (Critical → act now, Warning → act soon, Info → acknowledge).
3. Each alert gets a recommended action: `resolve` (issue addressed), `dismiss` (not actionable), `investigate` (needs root-cause analysis), or `escalate_to_human` (beyond curator authority).
4. The briefing includes the alert log cap status (count/cap, approaching flag) so the operator knows whether eviction is imminent.

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

### VERIFY — Confirm backlog cleared (step 5)

1. Call `curator_escalations` again to confirm the backlog is reduced.
2. Call `curator_clear_algedonic_log` to clear reviewed alerts from the in-memory log (frees the log before it evicts entries unread).
3. Render the `algedonic-review/verify-cleared` template to summarize what was resolved, what was dismissed, and what remains pending.
4. If the backlog is not reduced (operator declined to act), the skill notes this and exits — the alerts remain for the next review cycle.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `algedonic-review/triage-briefing.j2` | `KnowAct` | Structure the alert backlog into a triage list with per-alert severity, domain, and recommended action. |
| `algedonic-review/present-triage.j2` | `WordAct` | Render the triage list as a conversational summary with markdown tables. |
| `algedonic-review/execute-decisions.j2` | `KnowAct` | Produce a structured list of resolve/dismiss calls for operator-confirmed decisions. |
| `algedonic-review/verify-cleared.j2` | `WordAct` | Summarize what was resolved, dismissed, and what remains pending. |

## Constraints

- All templates run at `visibility: Public`.
- Human-in-the-loop: the skill proposes, the operator decides. The skill does NOT autonomously resolve or dismiss alerts.
- Ground every claim in the raw alert data. Do not fabricate alerts or severities not present in the inputs.
- If a signal channel returned an error or empty result, note it in the briefing — do not silently omit it.
- The in-memory algedonic log is a capped ring buffer (default 200 entries). The skill reviews the durable escalation queue, not the in-memory log — the in-memory log self-evicts when full. The `AlgedonicLogApproachingCap` signal is the trigger for running this skill, not a condition the skill itself clears.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.

## Design References

- Beer's Viable System Model — algedonic signals are the S1→S5 escalation path (System 1 pain → System 5 executive attention).
- Ashby's Law of Requisite Variety — the alert backlog is the variety the regulator could not absorb autonomously; human review is the external variety amplifier.
- Conant-Ashby theorem — "every good regulator of a system must be a model of that system." The triage briefing is the operator's model of the regulation system's state.
- Toyota Andon cord — algedonic alerts are the digital Andon; this skill is the structured response (not just acknowledgment).
- `docs/reports/gemba-loop-specification.md` — the gemba loop's Observe → Decide → Act phases map to this skill's TRIAGE → PRESENT → ACT phases.
