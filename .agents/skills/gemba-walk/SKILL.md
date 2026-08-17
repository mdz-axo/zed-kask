---
name: gemba-walk
core: true
visibility: public
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

### SENSE — Query algedonic log (step 1)

1. Call `curator_algedonic_log` with the configured lookback window (default 24 hours).
2. Algedonic alerts are pain/pleasure signals from the cybernetics loop — variety deficits, energy exhaustion, outcome plateaus.
3. If the query fails, the `on_failure: report` config reports the issue to the curator and resumes with a warning.

### GATHER — Query pending escalations (step 2)

1. Call `curator_escalations` to get the backlog of alerts requiring human review.
2. Escalations are alerts the cybernetics loop produced that need human attention — threshold breaches, drift detections, blocked actions.
3. If the query fails, the `on_failure: report` config reports the issue and resumes with a warning.

### GATHER — Query curator memory for skill performance (step 3)

1. Call `curator_consult` to query the curator's memory for skill performance patterns.
2. The curator ingests skill-use issue reports (via `curator_report_skill_use_issue`, stored as episodic h_mem with entity `skill_use_issue:<skill_name>`).
3. When `focus_skill` is set, the consultation is scoped to that skill; otherwise it asks for all skills with recent issues.
4. If the query fails, the `on_failure: report` config reports the issue and resumes with a warning.

### ANALYZE — Synthesize briefing (step 4)

1. Render the `gemba-walk/synthesize-briefing` template to structure the three signal channels into a coherent briefing.
2. The briefing has four sections: system health summary, algedonic alert digest, escalation backlog digest, per-skill performance digest.
3. Each per-skill entry includes issue count, recent failure patterns, and a health classification (healthy / watch / intervene).
4. The briefing explicitly notes when skill feedback spans (outcome, operator_feedback) are not available via MCP — this is a known gap, not a silent omission.

### PRESENT — Render briefing (step 5)

1. Render the `gemba-walk/present-briefing` template as a conversational summary with markdown tables.
2. The summary opens with a one-paragraph system health overview, then the algedonic alert table, then the escalation backlog table, then the per-skill performance table.
3. The presentation closes with a prompt for the operator to ask follow-up questions in the regular conversation.

### RECOMMEND — Propose actions (step 6)

1. Render the `gemba-walk/recommend-actions` template to propose refinement actions for operator approval.
2. For each skill with a "watch" or "intervene" classification, propose one of: `curator_directive`, `skill-maintenance`, `validate_golden_outputs`, `direct_edit`, or `no_action`.
3. The proposals are recommendations, not autonomous actions — the operator reviews and decides which to execute in the regular conversation.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `gemba-walk/synthesize-briefing.j2` | `KnowAct` | Structure the three signal channels (algedonic, escalations, memory) into a coherent briefing with per-skill health classification. |
| `gemba-walk/present-briefing.j2` | `WordAct` | Render the structured briefing as a conversational summary with markdown tables. |
| `gemba-walk/recommend-actions.j2` | `KnowAct` | Propose refinement actions (curator_directive, skill-maintenance, validate_golden_outputs, direct_edit, no_action) for operator approval. |

## Constraints

- All templates run at `visibility: Public`.
- Human-in-the-loop: the skill proposes, the operator decides. The skill does NOT execute refinement actions — it only recommends.
- Ground every claim in the raw signal data. Do not fabricate alerts, escalations, or skill issues that are not present in the inputs.
- If a signal channel returned an error or empty result, note it in the briefing — do not silently omit it.
- Skill feedback spans (outcome, operator_feedback, convergence) live in the in-memory RegulationLedger and are not exposed via MCP. The skill uses `curator_consult` as a proxy signal (skill-use issue reports are persisted to the curator's memory).
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.

## Design References

- Microsoft "Continuous improvement with agentic AI: Conducting a virtual gemba walk" — 7-step workflow maps to the cascade steps.
- Greenham "Gemba AI Framework" — TAPOIF lifecycle (Thought → Action → Pause → Observation → Inform → Follow-up) maps to the cascade.
- Meyer "The Gemba Was Always There" — flow kaizen, not point kaizen. The gemba walk makes the hidden flow visible.
- GembaCore — two-plane architecture (WorkPlane + OrchestrationPlane) maps to kask's separation of skill execution from regulation.
- C.H. Robinson "What Is Lean AI?" — start with a real problem, test solutions, integrate human oversight, measure results.
- `docs/reports/gemba-loop-specification.md` — the six-phase gemba loop specification.
