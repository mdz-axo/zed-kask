# Continuation Prompt: Gemba Walk Skill Design

## Context

This document is a continuation prompt for designing and implementing the
`gemba-walk` skill — the final remaining item from the Compiled AI gaps
work. The skill system infrastructure is complete; this is a creative
authoring task (designing the skill manifest and templates), not a code
wiring task.

## What's Already Built

The following infrastructure is wired and tested:

### Feedback signal channels (all emitting to `SkillSpanStore`)

| Span | Emission point | Payload |
|---|---|---|
| `reg.skill.<id>.outcome` | `BridgeManifestExecutor::execute_skill` after cascade | `{success, skill_id, exit_kind, resume_text?}` |
| `reg.skill.<id>.convergence` | `BridgeManifestExecutor::execute_skill` after cascade (success only) | `{iterations, exit_kind, converged}` |
| `reg.skill.<id>.operator_feedback` | `RecordSkillFeedbackTool` → `record_operator_feedback` | `{disposition, comments, skill_name}` |

### Drift detection (automated, in `MetacognitionLoop`)

- `sense_feedback_drift` trends both `outcome` success rate AND
  `operator_feedback` acceptance rate per skill
- Alerts via `FeedbackDrift { skill_id }` trigger when current window rate
  drops below `decline_ratio * prior_rate`
- Config: `feedback_drift_min_samples` (10), `feedback_drift_window` (10),
  `feedback_drift_decline_ratio` (0.8)

### Available tools for the gemba walk skill to call

- `curator_algedonic_log` — recent algedonic alerts
- `curator_status` — regulation health snapshot
- `curator_consult` — semantic + episodic memory consultation
- `curator_memory_recall` — entity-scoped memory recall
- `record_skill_feedback` — record operator feedback (the tool the gemba
  walk might recommend the operator use after reviewing)
- `validate_golden_outputs` — validate a skill's golden-output fixtures

### Query paths

- `RegulationLedger::query_skill_feedback(skill_id, phase)` — returns
  `Vec<StoredSkillSpan>` for a given skill and phase
- `RegulationLedger::skill_ids_with_feedback(phase)` — returns skill IDs
  that have spans for a given phase
- `MetacognitionLoop::last_snapshot_blocking()` — returns the last
  `HealthSnapshot` (variety deficit, critical alerts, effectiveness)

## Design References

The following sources inform the gemba walk skill design:

### 1. Microsoft Data Science — "Continuous improvement with agentic AI: Conducting a virtual gemba walk"
(https://medium.com/data-science-at-microsoft/continuous-improvement-with-agentic-ai-conducting-a-virtual-gemba-walk-b39836f16301)

Key patterns:
- **Hierarchical multi-agent pattern**: coordinator delegates to
  specialized agents (data collection, analysis, optimization)
- **7-step workflow**: problem definition → investigation plan → collect
  responses → summarize/analyze → virtual walk → recommendations →
  implementation tracking
- **Metrics**: process improvement efficiency, accuracy of insights,
  adoption rate, cost savings, bottleneck reduction, decision speed
- **Limitations**: lack of physical presence, dependence on data quality,
  potential resistance from employees

### 2. Chris Greenham — "Gemba AI Framework"
(https://www.linkedin.com/posts/chris-greenham_the-gemba-ai-way-to-design-and-deploy-agentic-activity-7349017996629004288-2W0c)

Key concepts:
- **GODO-IT, TEPUI, TAPOIF** — unified language for design
- **L1–L5 graduated maturity model** for autonomy
- **TAPOIF lifecycle**: Thought → Action → Pause → Observation → Inform →
  Follow-up
- **Three layers**: Strategic (purpose), Tactical (behavior), Operational
  (control)
- Governance spans from strategy to real-time telemetry

### 3. Kevin Meyer — "The Gemba Was Always There. We Just Couldn't See It."
(https://www.kevinmeyer.com/the-gemba-was-always-there-we-just-couldnt-see-it/)

Key insight:
- Knowledge work never had a real gemba — the work happened in people's
  heads, email threads, institutional memory
- AI systems with visibility into workflows may be the first technology
  capable of mapping the hidden flow
- **Flow kaizen, not point kaizen**: don't improve a step in the process —
  redesign the flow so wasteful steps don't need to exist
- The gemba walk reveals "the tax we pay for human cognitive limits" —
  coordination overhead disguised as legitimate activity

### 4. GembaCore/gemba-core
(https://github.com/GembaCore/gemba-core)

Key architectural patterns:
- **Two-plane architecture**: WorkPlane (data — work items, evidence) +
  OrchestrationPlane (agent sessions, dispatch)
- **Spec-driven development**: spec.md is human-authored intent, beads are
  operational work, reconciler bridges the two
- **Coach and Manager skills**: surface questions/blockers as escalations
- **Selection vs Planning**: dispatch-time scorer (which ready bead next)
  vs LLM-driven planning consult (which epics for sprint composition)
- **Gemba walk as "review of work in progress"** — a design doc concept,
  not a tool

### 5. C.H. Robinson — "What Is Lean AI?"
(https://www.chrobinson.com/en-us/resources/blog/what-is-lean-ai/)

Key principles:
- **Start with a real problem, not a theoretical one**
- **Test solutions, integrate human oversight, measure results**
- **Continuous human feedback loop**: employees review outcomes and
  fine-tune the system
- **40%+ productivity gain** from 30+ AI agents handling repetitive tasks
- Lean AI = Lean methodology (eliminate waste, optimize flow) + AI
  technology + domain expertise

## Design Constraints

The gemba-walk skill must:

1. **Be human-in-the-loop guided** — the Curator surfaces signals and
   proposes actions; the human operator makes refinement decisions. This
   is the kask design principle from the gemba loop specification.

2. **Use existing infrastructure** — no new MCP tools, no new storage, no
   new regulation code. The skill calls existing curator tools and
   synthesizes a briefing.

3. **Follow the six-phase gemba loop** from
   `docs/reports/gemba-loop-specification.md`:
   - Sense (automated, already running)
   - Prepare (Curator-assisted briefing — this is the skill's primary job)
   - Observe (interactive Q&A)
   - Decide (human)
   - Act (Curator executes approved actions)
   - Verify (automated, next cycle)

4. **Be a regular skill manifest** — not a new session type, not a new
   tool. The skill runs as a cascade via `BridgeManifestExecutor`.

5. **Respect the TAPOIF lifecycle** where applicable: the skill's cascade
   steps should map to Thought (analyze signals) → Action (prepare
   briefing) → Pause (await operator questions) → Observation (retrieve
   answers) → Inform (present findings) → Follow-up (track decisions).

## Proposed Skill Manifest Structure

```
gemba-walk
├── Step 1: SENSE — Query algedonic log + regulation health
│   ├── curator_algedonic_log (MCP tool)
│   └── curator_status (MCP tool)
├── Step 2: GATHER — Query skill feedback spans for all skills with activity
│   ├── For each skill_id from skill_ids_with_feedback("outcome"):
│   │   └── query_skill_feedback(skill_id, "outcome")
│   └── For each skill_id from skill_ids_with_feedback("operator_feedback"):
│       └── query_skill_feedback(skill_id, "operator_feedback")
├── Step 3: ANALYZE — Synthesize the briefing
│   └── LLM template: structure the signals into a readable briefing
│       with per-skill digest (invocations, success rate, acceptance rate,
│       convergence trends, drift alerts)
├── Step 4: PRESENT — Render the briefing to the operator
│   └── LLM template: conversational summary + structured table
├── Step 5: DISCUSS — Await operator questions
│   └── LLM template: answer questions by retrieving specific spans
└── Step 6: RECOMMEND — Propose actions for operator approval
    └── LLM template: for each skill with issues, propose:
        - curator_directive (threshold/budget adjustment)
        - skill-maintenance (artifact refinement)
        - validate_golden_outputs (for deterministic skills)
        - direct edit (manifest/template fix)
        - no action (observe further)
```

## Open Design Questions

1. **How does the skill enumerate skill IDs?** The
   `skill_ids_with_feedback` method is on `RegulationLedger`, which is not
   an MCP tool. The skill can't call it directly. Options:
   - Add a `curator_skill_feedback_summary` MCP tool that returns all
     skills with feedback and their aggregate stats
   - Have the Curator agent (which has access to the ledger via its
     internal tools) pre-compute the summary and pass it as context
   - The skill's step 1 calls `curator_status` which could be extended to
     include skill feedback summaries

2. **How does the skill handle the interactive Observe phase?** A skill
   cascade runs to completion — it can't pause for operator Q&A. Options:
   - The skill produces the briefing and recommendations in one pass, then
     the operator asks follow-up questions in the regular agent
     conversation (not within the skill cascade)
   - The skill is designed as a single-pass briefing generator, not an
     interactive session

3. **Should the skill record its own outcome?** Since
   `BridgeManifestExecutor::execute_skill` records outcome spans for every
   skill invocation, the gemba-walk skill itself will have outcome spans.
   This is correct — the gemba walk is itself a skill that can be tracked.

4. **Token budget?** The skill queries multiple data sources and
   synthesizes a potentially large briefing. The manifest should declare a
   generous gas budget to avoid mid-briefing exhaustion.

## Continuation Prompt

```
Design and implement the `gemba-walk` skill as a kask skill manifest
(`kask/registry/manifests/gemba-walk.yaml`) with its Jinja2 templates
(`kask/registry/templates/gemba-walk/`).

The skill implements the Prepare and Present phases of the gemba loop
(Phase 2 and 3 from docs/reports/gemba-loop-specification.md). It is a
single-pass briefing generator — not an interactive session. The operator
asks follow-up questions in the regular agent conversation after the skill
completes.

The skill's cascade:
1. Query algedonic log and regulation health via curator MCP tools
2. Query skill feedback spans (outcome + operator_feedback) — this
   requires either a new curator MCP tool (`curator_skill_feedback_summary`)
   or extending `curator_status` to include per-skill feedback aggregates.
   Decide which approach is simpler and implement it.
3. Synthesize a structured briefing with per-skill digest: invocation
   count, success rate, acceptance rate, convergence trends, drift alerts
4. Present the briefing as a conversational summary with a structured
   table per skill
5. Propose recommendations for operator action (curator_directive,
   skill-maintenance, validate_golden_outputs, direct edit, or no action)

Design constraints:
- Human-in-the-loop: the skill proposes, the operator decides
- Use existing infrastructure (no new storage, no new regulation code)
- The skill is a regular manifest cascade, not a new session type
- Follow the design references in docs/reports/gemba-walk-continuation-prompt.md
- Respect the .rules file (no unwrap(), no mod.rs, full words, etc.)
- The skill must carry {{ task }} in its context (the operator's gemba
  walk request, e.g., "show me the system health" or "review skill X")

The gemba walk concept: from Lean management, gemba (現場, "the actual
place") is the practice of going to where value is created to observe and
improve. In kask's digital context, the "actual place" is the running
cybernetic regulation system. The observer is the human operator with the
Curator as companion. The walk is a structured review session where the
operator and Curator jointly inspect feedback signals, identify
underperforming or drifting skills, and decide what refinement actions to
take.

Key insight from Kevin Meyer: "The gemba was always there. We just
couldn't see it." Knowledge work never had a real gemba — the work
happened in people's heads. AI systems with visibility into workflows may
be the first technology capable of mapping the hidden flow. The gemba-walk
skill makes the hidden flow visible.

Key insight from C.H. Robinson's Lean AI: "Start with a real problem, test
solutions, integrate human oversight, measure results." The gemba walk
starts with real signals (not theoretical concerns), proposes real actions
(not generic recommendations), and tracks results (via the outcome and
operator_feedback spans that the next gemba walk will review).

Key insight from Microsoft's virtual gemba walk: the 7-step workflow
(problem definition → investigation → collection → analysis → walk →
recommendations → tracking) maps to the skill's cascade steps. The
hierarchical multi-agent pattern (coordinator delegates to specialists)
maps to the skill calling specialized curator tools.

Key insight from GembaCore: the two-plane architecture (WorkPlane +
OrchestrationPlane) maps to kask's separation of skill execution
(WorkPlane — the cascades) from regulation (OrchestrationPlane — the
CyberneticsLoop + MetacognitionLoop). The gemba walk observes both planes.

After implementing the skill, add a test that verifies the manifest loads
and has the expected step structure (following the pattern in
hkask-templates/tests/yaml_schema_validation.rs).
```

## References

- `docs/reports/gemba-loop-specification.md` — the six-phase gemba loop spec
- `docs/reports/compiled-ai-gaps-review.md` — the revised plan with
  verification
- Microsoft: https://medium.com/data-science-at-microsoft/continuous-improvement-with-agentic-ai-conducting-a-virtual-gemba-walk-b39836f16301
- Greenham: https://www.linkedin.com/posts/chris-greenham_the-gemba-ai-way-to-design-and-deploy-agentic-activity-7349017996629004288-2W0c
- Meyer: https://www.kevinmeyer.com/the-gemba-was-always-there-we-just-couldnt-see-it/
- GembaCore: https://github.com/GembaCore/gemba-core
- C.H. Robinson: https://www.chrobinson.com/en-us/resources/blog/what-is-lean-ai/
