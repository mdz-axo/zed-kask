---
name: gemba-walk
core: true
description: "Human-in-the-loop guided review of the cybernetic regulation system. Queries algedonic alerts, pending escalations, and the curator's memory for skill performance patterns, then synthesizes a structured briefing with per-skill digest and proposed refinement actions for operator approval. Implements the Prepare and Present phases of the gemba loop.
"
---

# Gemba Walk

Human-in-the-loop guided review of the cybernetic regulation system. From Lean management, *gemba* (現場, "the actual place") is the practice of going to where value is created to observe and improve. In kask's digital context, the "actual place" is the running cybernetic regulation system. The observer is the human operator with the Curator as companion. The walk is a structured review session where the operator and Curator jointly inspect feedback signals, identify underperforming or drifting skills, and decide what refinement actions to take.

The skill implements the Prepare and Present phases of the six-phase gemba loop (Sense → Prepare → Observe → Decide → Act → Verify). It is a single-pass briefing generator — not an interactive session. The operator asks follow-up questions in the regular agent conversation after the skill completes (the Observe phase happens outside the skill cascade).

## When to Use

- The operator wants a structured review of the regulation system's current health.
- The operator wants to identify skills with recurring issues or drift patterns.
- The operator wants a briefing before deciding which refinement actions to take.
- The operator wants to triage pending escalations and algedonic alerts.
- The operator wants to focus on a specific skill's performance (use `focus_skill` input).

## Instructions

### SENSE+GATHER — Query five curator signal channels (step 1)

1. A single `mcp_batch` step queries five independent channels concurrently: `curator_algedonic_log` (default 24h lookback), `curator_escalations`, `curator_consult` (skill performance patterns, scoped to `focus_skill` when set), `curator_grounding_trend`, and `curator_grounding_coverage`.
2. Algedonic alerts are pain/pleasure signals from the cybernetics loop — variety deficits, energy exhaustion, outcome plateaus. Grounding trend/coverage come from the verification ledger (clean_rate, coverage gaps).
3. If any channel fails, the `on_failure: report` config reports the issue and the synthesis proceeds with whichever results are available; missing channels are noted as gaps.

### ANALYZE — Synthesize briefing (step 2)

1. Render the `gemba-walk/synthesize-briefing` template to structure the five signal channels into a coherent briefing.
2. The briefing has five sections: system health summary, algedonic alert digest, escalation backlog digest, per-skill performance digest, grounding health digest (clean_rate, coverage_rate, trend direction, top coverage gaps).
3. Each per-skill entry includes issue count, recent failure patterns, and a health classification (healthy / watch / intervene).
4. The briefing explicitly notes when skill feedback spans (outcome, operator_feedback) are not available via MCP — this is a known gap, not a silent omission.

### PRESENT — Render briefing (step 3)

1. Render the `gemba-walk/present-briefing` template as a conversational summary with markdown tables.
2. The summary opens with a one-paragraph system health overview, then the algedonic alert table, then the escalation backlog table, then the per-skill performance table, then the grounding health table.
3. The presentation closes with a prompt for the operator to ask follow-up questions in the regular conversation.

### RECOMMEND — Propose actions (step 4)

1. Render the `gemba-walk/recommend-actions` template to propose refinement actions for operator approval.
2. For each skill with a "watch" or "intervene" classification, propose one of: `curator_directive`, `skill-maintenance`, `validate_golden_outputs`, `direct_edit`, or `no_action`.
3. Additionally, propose grounding-specific actions: register a contract for agent types with delegations but no contract, review recent violations when the clean rate dropped, or investigate narrative leaks.
4. The proposals are recommendations, not autonomous actions — the operator reviews and decides which to execute in the regular conversation.

### CONVERGE — Deterministic check (step 5) and loop (step 6)

1. A `lisp.eval` compute step extracts the `briefing_complete` flag from the synthesize step as the convergence signal (1 = complete, 0 = incomplete; absent = 0 — surfaces the failure rather than masking it).
2. If not converged, the loop re-enters at step 2, bounded by `max_iterations: 3` — a gemba walk that cannot produce a briefing in 3 passes escalates.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `synthesize-briefing.j2` | KnowAct | Structure the five signal channels (algedonic, escalations, memory, grounding trend, grounding coverage) into a coherent briefing with per-skill health classification (healthy / watch / intervene) and a grounding health digest (clean_rate, coverage_rate, trend direction, top coverage gaps). |
| `present-briefing.j2` | WordAct | Render the structured briefing as a conversational summary with markdown tables: system health overview, algedonic alert table, escalation backlog table, per-skill performance table, grounding health table. Closes with a prompt for the operator to ask follow-up questions in the regular conversation. |
| `recommend-actions.j2` | KnowAct | Propose refinement actions for operator approval. For each skill with a "watch" or "intervene" classification, propose one of: curator_directive, skill-maintenance, validate_golden_outputs, direct_edit, or no_action. Additionally proposes grounding-specific actions: register a contract for agent types with delegations but no contract, review recent violations, or investigate narrative leaks. Recommendations, not autonomous actions. |

## Constraints

- All templates run at `visibility: Public`.
- Human-in-the-loop: the skill proposes, the operator decides. The skill does NOT execute refinement actions — it only recommends.
- Ground every claim in the raw signal data. Do not fabricate alerts, escalations, or skill issues that are not present in the inputs.
- If a signal channel returned an error or empty result, note it in the briefing — do not silently omit it.
- Skill feedback spans (outcome, operator_feedback, convergence) live in the in-memory RegulationLedger and are not exposed via MCP. The skill uses `curator_consult` as a proxy signal (skill-use issue reports are persisted to the curator's memory).
- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.

## Design References

- Microsoft "Continuous improvement with agentic AI: Conducting a virtual gemba walk" — 7-step workflow maps to the cascade steps.
- Greenham "Gemba AI Framework" — TAPOIF lifecycle (Thought → Action → Pause → Observation → Inform → Follow-up) maps to the cascade.
- Meyer "The Gemba Was Always There" — flow kaizen, not point kaizen. The gemba walk makes the hidden flow visible.
- GembaCore — two-plane architecture (WorkPlane + OrchestrationPlane) maps to kask's separation of skill execution from regulation.
- C.H. Robinson "What Is Lean AI?" — start with a real problem, test solutions, integrate human oversight, measure results.
- `docs/reports/gemba-loop-specification.md` — the six-phase gemba loop specification.
