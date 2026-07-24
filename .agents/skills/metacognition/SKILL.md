---
name: metacognition
visibility: public
description: "Master self-reflection skill. Reflects on a context and sees it from different perspectives — decomposes goals, self-assesses progress, detects ellipses (what is missing) via Bloom's method, rotates perspectives via the Falstaffian engine, and calibrates strategy. Improves itself through GEPA evolutionary optimization (gpa-evolution). Composes sequential-inquiry, pragmatic-laziness, and hypothesis-framer. Any userpod may invoke this skill."
---

# Metacognition

Master self-reflection skill. Any userpod — curator, bot, agent, or ensemble member — can invoke metacognition to reflect on a context and see it from different perspectives. The skill decomposes a goal into sub-goals, self-assesses progress from multiple angles, detects ellipses (what is missing) via Harold Bloom's method, rotates perspectives via the Falstaffian engine, calibrates strategy based on prior outcomes, and produces a concrete next experiment. The skill improves itself through GEPA (Genetic-Pareto) evolutionary optimization via the `gpa-evolution` skill.

## When to Use

- You need to decompose a complex goal and assess whether your current approach is working
- You want to see a context from different perspectives before committing to action
- You suspect something is missing from your understanding — gaps, omissions, unstated assumptions
- You need to calibrate your strategy, effort estimates, or tool selection based on what you've learned
- You want to produce a testable next experiment (PDCA) with clear success criteria
- You want to improve your own metacognitive process through evolutionary optimization
- You need to frame a research question formally (delegates to `hypothesis-framer`)
- You need to find the path of least action through a complex situation (delegates to `pragmatic-laziness`)
- You need multi-step reasoning with branching and hypothesis testing (delegates to `sequential-inquiry`)
- When confidence is low (< 0.5) and standard evidence-gathering has plateaued — the Calibrate step maps `uncertainty` obstacles to strategy adjustments (iterative_refinement, depth_first, breadth_first) and the Falstaffian engine rotates perspective to find non-obvious paths to higher certainty
- When uncertainty is a first-class obstacle that needs classification (knowledge_gap, perspective_blind, context_loss, conflict) and routing to the appropriate certainty-finding tool — metacognition is the master uncertainty router

## PDCA Loop

The skill follows a **Plan → Do → Check → Act** cycle with a post-convergence improvement step:

```
Plan:   Step 1 — Decompose     → Break goal into sub-goals with dependencies and effort estimates
Do:     Step 2 — Assess        → Self-assess current vs. target condition, identify obstacles
Do:     Step 3 — Ellipsis      → Detect what is missing (Bloom method: ellipsis vs. leak)
Do:     Step 4 — Calibrate     → Adjust strategy, select Falstaffian shape, produce next experiment
Check:  Step 5 — Converge      → Self-assessment stability detection
Act:    Step 6 — GEPA Improve → Evolve the metacognition process itself (gpa-evolution)
Act:    Step 7 — Loop          → If not converged, re-enter at Step 1 with refined goal
```

## Improvement Measure

### Convergence Metric: Self-Assessment Stability Detection

**Field**: `step_5_result.convergence_metric`

The convergence metric measures whether the userpod's self-assessment and calibration are sufficiently stable to act on. Computed by the `meta-convergence-check.j2` template:

| Score | Meaning |
|-------|---------|
| 0.00 | Decomposition is acyclic, assessment grounded, all obstacles addressed, experiment specific — converged |
| 0.25 | Assessment adequate for action, minor uncertainties remain — converged at threshold |
| 0.50 | Assessment vague, calibration disconnected from obstacles — not converged |
| 1.00 | No meaningful self-assessment performed — not converged |

**Threshold**: 0.25. **Max iterations**: 5.

**Scoring breakdown** (start at 1.0, subtract for each satisfied check):

1. Decomposition: goal broken into acyclic sub-goals with effort estimates? → +0.20 if missing
2. Assessment: actual condition grounded in evidence? → +0.20 if vague
3. Obstacles: specific, typed, and severity-rated? → +0.20 if vague
4. Calibration: addresses the identified obstacles? → +0.20 if disconnected
5. Next experiment: small, testable, and measurable? → +0.10 if vague
6. Confidence: calibrated (not overconfident)? → +0.10 if unrealistic

### GEPA Self-Improvement (Post-Convergence)

After convergence, the skill invokes the `gpa-evolution` skill to evolve its own templates and strategies:

1. **Sample trajectories** — Capture execution trajectories from the current metacognition cycle (input, decomposition, assessment, calibration, outcome scores).
2. **Reflect in natural language** — Diagnose why the assessment was accurate or inaccurate, surface transferable rules about when to trust self-assessment, identify success and failure patterns. This reflection IS the gradient signal — it replaces sparse scalar rewards with rich, actionable prose.
3. **Propose mutations** — Generate 3–7 variants of the metacognition templates (meta_decompose, meta-assess, meta-calibrate) via targeted edits and crossover from non-dominated frontier members. Each variant tests one hypothesis: "if I change the assessment prompt to ask about X, then confidence calibration will improve because Y."
4. **Test variants** — Execute each mutated template against the eval set and collect per-objective scores (accuracy, calibration, actionability) plus cost.
5. **Update Pareto frontier** — Merge current frontier with tested variants, keep non-dominated members, prune by crowding distance.
6. **Check convergence** — Pareto-frontier stability (hypervolume delta + new non-dominated members). Converged when the frontier stops moving.

## Composed Skills

Metacognition is a **master skill** that composes other skills for deeper analysis:

| Skill | Role | When Invoked |
|-------|------|-------------|
| `gpa-evolution` | Self-improvement | Post-convergence — evolves metacognition's own templates through GEPA |
| `sequential-inquiry` | Deep analysis | On-demand — when assessment reveals a subproblem requiring multi-step reasoning with branching and hypothesis testing |
| `pragmatic-laziness` | Path optimization | On-demand — when calibration identifies wasted effort, broken feedback loops, or unnecessary complexity |
| `hypothesis-framer` | Hypothesis formation | On-demand — when the next experiment needs a formally testable hypothesis with FINER + PICO structuring |

### Composition Protocol

1. **Decompose first** — Always start with meta_decompose to break the goal into sub-goals.
2. **Assess honestly** — Use meta-assess with evidence from session history and prior outcomes.
3. **Detect ellipses** — If context_text is provided, apply Bloom's method to find what is missing.
4. **Delegate when needed** — If the assessment reveals a subproblem that needs:
   - Multi-step reasoning → delegate to `sequential-inquiry`
   - Least-action path finding → delegate to `pragmatic-laziness`
   - Formal hypothesis → delegate to `hypothesis-framer`
5. **Calibrate with perspective** — Apply Falstaffian shape rotation to reframe the calibration.
6. **Improve after convergence** — Invoke `gpa-evolution` to evolve the process.

## Ellipsis Detection (Bloom Method)

The skill uses the `ellipsis-analysis.j2` template to detect meaning in gaps and omissions. Harold Bloom's five-step method:

1. **Read deeply** — Not to believe, accept, or contradict, but to learn. Engage with the context as a program reaching through time.
2. **Mind the gaps** — What is said AND what is not said. Compare against expected elements, topics, and metrics.
3. **Differentiate ellipsis from leak**:
   - **Ellipsis**: Deliberate omission that creates meaning. Reader fills the gap. Adding it would diminish the text.
   - **Leak**: Unintentional information loss. Causes confusion or errors. Adding it would fix an error.
4. **See past yourself** — Reject received narratives. Check biases. Question assumptions. Calibrate against mediocrity.
5. **Find what is not inferno** — Seek the exceptional, idiosyncratic, and excellent. Give it space. "We owe mediocrity nothing."

### Ellipsis vs. Leak Decision Tree

```
Is this literary/artistic?     → Likely ellipsis (artistic omission creates meaning)
Is this technical/documentation? → Likely leak (technical omission causes errors)
Does the gap create meaning?    → Ellipsis
Does the gap cause confusion?   → Leak
Would adding it diminish?       → Ellipsis
Would adding it fix?             → Leak
```

## Falstaffian Perspective Rotation

The skill uses the Falstaffian perspective engine to see the context from different angles. The engine is a three-fold structure:

### Three-Fold Structure

| Dimension | Role | Nature |
|-----------|------|--------|
| **Shapes** | Rotation vectors | Abstract semantic transformations that rotate perspective |
| **Experience** | Calibrated wisdom | Knows what comes (betrayal, rejection, death) — not bitter, not naive |
| **Spirit** | Life affirmation | Affirms despite knowledge — love, play, hope, youthfulness |

The genius is the **tension** between experience and spirit. Shapes carry this tension without breaking into bitterness (experience alone) or naivety (spirit alone).

### Seven Falstaffian Shapes

| Shape | Confidence | Abstract Form | When to Apply |
|-------|-----------|---------------|---------------|
| **Predicate Hollow** | High (3/3) | Abstract predicate applied to subject → examine predicate's ontology → predicate is hollow | When an abstract quality (honor, courage, intelligence) is invoked critically |
| **Subject Expansion** | High (3/3) | Individual criticized → expand to universal → the criticism applies to all, not just the individual | When an individual is criticized for a universal trait |
| **Value Redefinition** | High (3/3) | Elite value invoked → redefine what matters → the elite value is hollow, real value is elsewhere | When elite values (status, reputation) are used to judge |
| **Object Inversion** | High (3/3) | Hierarchical object valued → invert the hierarchy → the lowly object has more value | When a hierarchy is used to rank value |
| **Direction Reversal** | Medium (2/3) | Top-down evaluation → reverse direction → evaluate the evaluator | When a top-down judgment is made without self-examination |
| **Cultural Authority** | Medium (2/3) | Cultural assumption invoked → question the cultural authority → the assumption is a convention, not truth | When cultural norms are invoked as authority |
| **Linguistic Precision** | Medium (2/3) | Moral wordplay possible → expose the wordplay → the moral term is a pun, not a principle | When moral language is used ambiguously |

### Shape Selection Decision Tree

```
abstract_predicate_applied    → predicate_hollow
individual_criticism          → subject_expansion
elite_value_invoked           → value_redefinition
hierarchical_object_valued    → object_inversion
top_down_evaluation           → direction_reversal
cultural_assumption_invoked   → cultural_authority
moral_wordplay_possible       → linguistic_precision
```

### Integration with Ellipsis

| Shared Concept | Ellipsis | Falstaffian | Combined |
|---------------|----------|-------------|----------|
| Meaning in what is absent | Gap detection | Gap as tension space | Absence is where tension lives |

## Instructions

### 1. Decompose

1. Break the high-level goal into a tree of sub-goals with IDs, descriptions, effort estimates (0.0–1.0), dependencies, and tool recommendations.
2. Select an execution strategy: `sequential_execution`, `breadth_first`, `depth_first`, or `iterative_refinement`.
3. Effort estimates must sum to approximately 1.0 (within ±0.1). The dependency graph must be acyclic.
4. Use the `meta_decompose.j2` template with `goal`, `session_history`, `available_tools`, and `prior_outcomes`.

### 2. Assess

1. Define the **target condition** — what does success look like? Be specific and measurable.
2. Define the **actual condition** — what is your current state? Be honest, not optimistic. Ground in evidence from session history and prior outcomes.
3. Identify **obstacles** — each with type (knowledge_gap, tool_limitation, resource_constraint, dependency_block, uncertainty, complexity), description, severity (low/medium/high), and mitigation.
4. Estimate **progress** (0.0–1.0) and **confidence** (0.0–1.0). Low confidence means you need more information.
5. Identify **blockers** — hard blockers requiring external intervention.
6. Use the `meta-assess.j2` template with `goal`, `decomposition`, `current_state`, `session_history`, and `prior_outcomes`.

### 3. Detect Ellipses (if context provided)

1. This step runs only when `context_text` is provided (condition: `context_text | length > 0`). When no context text is provided, the step is skipped.
2. Apply Bloom's five-step method to the `context_text`:
   - **Read deeply** — engage with the text to learn, not to judge.
   - **Mind the gaps** — compare against `expectations`. What is expected but absent?
   - **Differentiate** — classify each gap as ellipsis (deliberate, meaningful) or leak (unintentional, harmful).
   - **See past yourself** — check biases (`optimism_bias`, `confirmation_bias`, `anchoring`). Question assumptions (`goal_is_clear`, `tools_are_sufficient`, `context_is_complete`).
   - **Find what is not inferno** — seek the exceptional in the context. Give it space.
2. Use the `ellipsis-analysis.j2` template with `text`, `expectations`, `biases`, `assumptions`, and `domain`.
3. The ellipsis findings (gaps classified as ellipsis or leak) are passed to step 4 (calibration) as `ellipsis_findings` input.
4. If no `context_text` is provided, skip this step — calibration proceeds without ellipsis findings.

### 4. Calibrate with Perspective Rotation

1. **Review the assessment and ellipsis findings** — what obstacles remain? What was missing? The `meta-calibrate.j2` template receives both `assessment` (from step 2) and `ellipsis_findings` (from step 3) as inputs.
2. **Map ellipsis findings to obstacles** — for each ellipsis (deliberate omission), determine if it reveals a strategic blind spot. For each leak (unintentional omission), determine if it reveals a knowledge gap.
3. **Select a Falstaffian shape** using the decision tree (see below). The `perspectives` input controls which perspectives are applied. Default: `['falstaffian', 'ellipsis']`.
   - If an abstract quality is being criticized → **Predicate Hollow** (examine whether the quality is hollow)
   - If an individual is criticized for a universal trait → **Subject Expansion** (the criticism applies to all)
   - If elite values are used to judge → **Value Redefinition** (what really matters?)
   - If a hierarchy ranks value → **Object Inversion** (invert the hierarchy)
   - If a top-down judgment is made → **Direction Reversal** (evaluate the evaluator)
   - If cultural norms are invoked → **Cultural Authority** (convention vs. truth)
   - If moral language is ambiguous → **Linguistic Precision** (expose the wordplay)
3. **Apply the shape** to reframe the calibration — see the obstacle from the rotated perspective. The tension between experience (what we know will happen) and spirit (what we affirm anyway) produces wisdom, not bitterness.
4. **Adjust strategy** based on evidence:
   - Low progress + knowledge gaps → `iterative_refinement` or `depth_first`
   - Moderate progress + on track → maintain `sequential_execution`
   - High progress + low confidence → `breadth_first` to validate
   - Blockers present → escalate or find alternative path
5. **Recalibrate effort** estimates based on prior outcomes (increase for harder-than-expected, decrease for easier).
6. **Select tools** — map obstacles to specific MCP tools (knowledge_gap → codegraph, memory; tool_limitation → alternative tools; resource_constraint → rebalance gas; dependency_block → escalate; uncertainty → small experiment; complexity → decompose further).
7. **Define the next experiment** — a small, testable PDCA step with hypothesis, action, measurement, and success criteria.
8. Use the `meta-calibrate.j2` template with `goal`, `decomposition`, `assessment`, `prior_outcomes`, and `available_tools`.

### 5. Check Convergence

1. Evaluate whether decomposition, assessment, obstacles, calibration, and next experiment are stable.
2. Compute the convergence metric using the scoring guidance in the Improvement Measure section.
3. If converged (≤ 0.25), proceed to act.
4. If not converged, identify the specific gap and re-enter the cycle.

### 6. GEPA Self-Improvement (Optional, Post-Convergence)

1. After convergence, optionally invoke the `gpa-evolution` skill to evolve the metacognition process. The `meta-gepa-improve.j2` template wraps this delegation — it adapts metacognition cycle trajectories to GEPA's expected inputs, interprets the Pareto frontier results back into metacognition recommendations, and handles the case where GEPA is not available (returns `improvement_invoked: false`). This is a genuine transformation, not a pass-through wrapper.
2. Sample trajectories from the current cycle (input → decomposition → assessment → ellipsis → calibration → outcome).
3. Reflect in natural language: Was the decomposition accurate? Was the assessment grounded? Did the calibration address the right obstacles? What would make the next cycle better?
4. Propose mutations to the metacognition templates — each tests one hypothesis about what would improve the process.
5. Test variants and update the Pareto frontier.
6. The evolved templates become the default for future metacognition invocations.

### 7. Act / Loop

1. If converged, the calibrated action plan (strategy, effort estimates, tool selection, next experiment) is ready for execution.
2. If not converged after 5 iterations, escalate.
3. The next experiment from Step 4 is the immediate action — execute it, measure the outcome, and feed the result back as a `prior_outcome` for the next metacognition cycle.

## Registry Templates

| Template | Type | Purpose |
|----------|------|--------|
| `meta_decompose.j2` | `KnowAct` | Decompose goal into sub-goals with dependencies, strategy, and effort estimates. |
| `meta-assess.j2` | `KnowAct` | Self-assess current vs. target condition, identify obstacles, estimate progress and confidence. |
| `ellipsis-analysis.j2` | `KnowAct` | Bloom Method: detect meaning in gaps and omissions. Differentiate ellipsis (deliberate) from leak (unintentional). |
| `meta-calibrate.j2` | `KnowAct` | Calibrate strategy, effort, and tools based on assessment and prior outcomes. Produces PDCA next experiment. |
| `meta-gepa-improve.j2` | `KnowAct` | GEPA self-improvement — delegates to gpa-evolution skill to evolve metacognition templates through Genetic-Pareto optimization. |
| `meta-convergence-check.j2` | `KnowAct` | Compute convergence metric via self-assessment stability detection. |
| `falstaffian-perspective-engine.yaml` | `KnowAct` | Reference: three-fold structure (shapes, experience, spirit) with metacognitive application steps. |
| `falstaffian-shapes.yaml` | `KnowAct` | Reference: seven semantic graph transformation operators with input/output structures. |
| `falstaffian-variance-analysis.yaml` | `KnowAct` | Reference: three-pass variance calibration with agreement matrix and final taxonomy. |

## Fusion Mode

This skill supports **fusion mode** via the `fusion:` block in its flow manifest.
When enabled, all analysis steps route through a multi-model panel — either with
LLM judge synthesis or the **algo / no-judge** path (`judge: algo`) for deterministic
JSON merge without an LLM judge call. This skill uses **synthesis mode** — Compose
perspectives matches metacognitive integration.

The convergence check step has `fusion: false` to ensure deterministic rubric
evaluation uses single-model inference.

## Constraints

- All flow templates are `KnowAct` type with `Public` visibility.
- Energy caps: discover (6000), assess (4096), ellipsis (4096), calibrate (6144), convergence-check (2000), GEPA improvement (8192).
- Gas cap: 150,000 per invocation. Maximum 5 iterations.
- Be honest, not optimistic — overestimating progress is worse than underestimating.
- Ground assessments in evidence from session history and prior outcomes.
- Effort estimates must sum to ~1.0 (±0.1). Dependency graph must be acyclic.
- The next experiment must be small and testable, not a large commitment.
- Falstaffian shape selection must match the context — don't force a shape that doesn't fit.
- GEPA self-improvement is optional — it costs gas and should be invoked when the userpod wants to improve, not on every cycle.
- Jinja2 sandboxed execution: no arbitrary Python code when safety mode is enabled.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.