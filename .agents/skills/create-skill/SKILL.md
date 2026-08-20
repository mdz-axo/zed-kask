---
name: create-skill
core: true
description: "Create a new kask skill as a complete registry crate: manifest.yaml + .j2 templates + process manifest + SKILL.md companion. Overrides the built-in Zed create-skill, which only produces SKILL.md files."
---

# Create Skill (kask-native)

Create a new kask skill as a complete registry crate, grounded in the
skill's specific domain. This skill overrides the built-in Zed
`create-skill`, which only produces SKILL.md files — insufficient for
kask, where the SKILL.md is a discovery-only catalog entry and the actual
implementation lives in the registry (`kask/registry/`).

## Core principle: idiosyncratic, not generalized

Each skill is customized and idiosyncratic to its domain. The skill's
PDCA shape emerges from its ontological anchors — the academic and
industry processes that define how the domain works. A gradient-hunter
follows Prior → Map → Detect → Hypothesize → Report → Convergence because
that's what gradient analysis IS (wombling, RDD, Rubin, persistent
homology, spin glasses, allostery). A bug-hunt follows Charter → Probe →
Oracle → Taxonomize → Report → Convergence because that's what
exploratory testing IS (Hendrickson, Bach, Weinberg, Beizer). A
self-improvement skill follows a nested PDCA + outer Kata because that's
what self-induced update IS (Ren et al. 2026, Toyota Improvement Kata).

**Do not generalize the shape.** Copying the bug-hunt pattern for a
non-bug-hunt skill produces a hollow skill — the shape without the
anchors. The create-skill process finds the anchors first, then lets the
shape emerge.

## When to Use

- You need to create a new kask skill from a natural-language description
- You need to scaffold the full registry crate structure with ontological grounding
- You need to override the built-in `create-skill` with kask-native registry semantics

Do NOT use for:
- Creating a simple SKILL.md-only skill (use the built-in `create-skill` — but note this produces a registry-incomplete skill)
- Validating an existing skill (use `skill-maintenance-validate`)
- Auditing skill health (use `skill-maintenance-audit`)

## The kask skill structure

Every kask skill has artifacts in four locations:

```
1. .agents/skills/<name>/SKILL.md                    # Catalog entry (discovery)
2. kask/registry/templates/<name>/manifest.yaml      # Template manifest
3. kask/registry/templates/<name>/*.j2               # Jinja2 templates
4. kask/registry/manifests/<name>.yaml               # Process manifest (FlowDef)
```

The SKILL.md is the *interface* — Zed's discovery machinery parses its
frontmatter (name + description) to surface it in the slash-command catalog
and the model's `<available_skills>` list. The *implementation* (the YAML
manifest in `kask/registry/manifests/`) is the process manifest that the
ManifestExecutor dispatches.

## Ontological anchoring

Every skill is grounded in ontological anchors — the academic and industry
processes that define how the skill's domain works. The create-skill
process finds these anchors in the research phase and embeds them in the
skill's artifacts.

### Ontology reference set

The create-skill process selects from (and extends) this ontology set
based on the skill's domain:

| Ontology | Domain | Use when |
|---|---|---|
| **PKO** (Procedural Knowledge Ontology, Carriero et al. 2025) | Industrial processes, procedures, workflows | The skill models a procedure with specification/execution separation |
| **Dublin Core** | Metadata, documentation, cataloging | The skill produces or manages metadata artifacts |
| **GOLEM** (Graphs and Ontologies for Literary Evolution Models, Pianzola et al. 2024) | Narrative, fiction, storytelling | The skill models narrative structure, characters, events, settings |
| **ESO** (Event and Implied Situation Ontology, Segers et al. 2015) | Event structures, scientific inquiry | The skill models events with pre/post situations and entity roles |
| **Domain-specific** | Any | The research phase finds ontologies specific to the skill's domain (e.g., spin glass theory for gradient-hunter, Beizer taxonomy for bug-hunt, Ren et al. for self-improvement) |

The ontology selection is not exhaustive — the research phase may find
additional ontologies. The point is that the skill's artifacts should be
annotated with the ontology terms that define its domain's process.

### How ontological anchoring shapes the skill

1. **PDCA shape**: the ontology's process structure implies the skill's
   phase structure. PKO's specification/execution separation implies a
   Plan → Execute → Verify shape. ESO's pre/post situations imply a
   Before → Event → After shape. The gradient-hunter's eight gradient
   ontologies imply a Prior → Map → Detect → Hypothesize shape.
2. **Template contracts**: the ontology's entity types become the
   template contract's input/output types. PKO's Procedure, Step,
   StepExecution become contract fields. ESO's Event, Situation, Role
   become contract fields.
3. **Span namespaces**: the ontology's concepts become span names. There are
   two distinct namespaces — do not conflate them:
   - `ledger.span_namespace` (process manifest): MUST be `reg.skill.<manifest.id>`
     (CI-enforced by `scripts/check-skill-span-namespace.sh`; the `spans:` list is
     abolished). E.g. `reg.skill.bug-hunt`.
   - per-template `generates_spans` (template manifest + .j2): the ontology-derived
     short name, e.g. `reg.gradient.detect` follows the gradient ontology,
     `reg.bughunt.oracle` follows the Weinberg oracle concept. These are NOT gated
     by the CI script and may use a shortened form distinct from `manifest.id`.
4. **Convergence criteria**: the ontology's quality criteria become the
   convergence metric. PKO's execution completeness becomes a coverage
   metric. The gradient-hunter's fractal recurrence becomes a
   stabilization metric.

## PDCA Loop

The create-skill process itself follows a PDCA loop, but the shape is
specific to skill creation:

```
Plan:   Phase 1 — Research     → Find academic/industry ontological anchors for the skill's domain
Plan:   Phase 2 — Describe     → Capture purpose, name, PDCA shape (emergent from anchors), delegates, ontology
Do:     Phase 3 — Scaffold    → Generate manifest.yaml + .j2 templates + process manifest + SKILL.md
Check:  Phase 4 — Validate    → Run skill-maintenance-validate against R1-R12, Z1-Z8, X1-X4, E1-E11
Check:  Phase 5 — Converge     → Check validation passed; if not, re-enter at Research with fixes
Act:    Phase 6 — Loop        → If validation failed, re-enter at Phase 1 with the failure report
```

The research phase is first because the anchors determine everything else.
Without research, the describe phase would default to a generic shape —
which is exactly what we want to avoid.

## Composed Skills

| Skill | Role | When Invoked |
|-------|------|-------------|
| `skill-maintenance` | Scaffolding + validation | Phase 3 (scaffold via `skill-maintenance-build`) and Phase 4 (validate via `skill-maintenance-validate`) |

## Instructions

### Phase 1 — Research (find ontological anchors)

1. Search the academic and industry literature for the skill's domain.
   Use web search and academic search tools to find:
   - **Process ontologies**: how does the domain's process work? What
     are the canonical phases, steps, or stages?
   - **Quality criteria**: how does the domain measure success? What
     are the convergence criteria?
   - **Entity types**: what are the domain's objects, events, roles,
     situations?
   - **Existing ontologies**: is there a PKO, Dublin Core, GOLEM,
     ESO, or domain-specific ontology that formalizes
     this domain?
2. Record the ontological anchors: for each anchor, cite the source
   (author, year, paper/standard) and describe how it shapes the skill.
3. Select the ontology reference set: which ontologies will annotate the
   skill's artifacts?
4. Derive the PDCA shape from the anchors: what phases does the skill
   need? The shape emerges from the domain's process, not from a
   generic template.

### Phase 2 — Describe (capture specification)

1. Capture the skill's purpose from the user's natural-language
   description, informed by the research phase.
2. Choose a name: lowercase, hyphenated, 2-40 characters, verb-noun or
   noun-noun, no reserved prefixes.
3. Specify the PDCA phases — these are the phases that emerged from the
   research phase, not a generic template. Each phase is grounded in an
   ontological anchor.
4. Identify which skills this skill will compose (delegates).
5. Identify the per-template span namespace (`reg.<skill_name>.<phase>`,
   ontology-derived). The ledger `span_namespace` is deterministic — always
   `reg.skill.<manifest.id>` — and is injected by the scaffold phase; do not
   "choose" it.
6. Identify the ontology annotations for each artifact.

### Phase 3 — Scaffold (delegate to skill-maintenance-build)

Delegate to `skill-maintenance-build` to generate the full registry crate:

1. **Template manifest** with ontology-annotated template entries
2. **.j2 templates** with contracts whose types come from the ontology
3. **Process manifest** with the idiosyncratic PDCA shape (not a generic
   template), rjoule/convergence blocks, OCAP capabilities
4. **SKILL.md companion** with ontology references in the description
   and constraints

### Visual artifact surfacing (Phase 3 gate)

If the skill produces a visual artifact (Mermaid diagram, chart, map, or any
renderable output) in an intermediate `select` step, the process manifest **must
include a final `render` step** that surfaces the artifact as the cascade's
final output. Without it, the artifact stays buried in an intermediate
`step_N_result` and `extract_final_step_result` picks a later step (compute/loop)
that has no diagram — the user never sees the visualization.

The pattern:

1. An intermediate `select` step generates the visual artifact source (e.g.,
   `map_diagram` containing `quadrantChart ...`, or `mermaid_source` containing
   `sankey-beta ...`).
2. A final `render` step (RenderAct, `action: render`) takes the artifact source
   via `input_mapping` and wraps it in a fenced ```mermaid block as a markdown
   string. This step is deterministic (no LLM call, `timeout_seconds: 10`).
3. The `loop` step comes **after** the render step. The render step's ordinal
   must be the highest among steps that produce a `step_N_result` (the `loop`
   action does not produce one), so `extract_final_step_result` picks it.
4. The render template is a pure Jinja2 file (no `[inference]` frontmatter) — the
   `render` action calls `render_minijinja` on the full file content, so
   frontmatter would appear in the output.

Detection criteria — add a render step if **any** of these are true:
- A template's `contract.output` includes a field whose description says "mermaid",
  "diagram", "chart", "graph", "visual", or "plot".
- A template instructs the model to "generate a Mermaid ... chart/diagram".
- The skill's SKILL.md description mentions "renders natively in Zed", "visual",
  "diagram", or "chart".

Skills that do NOT produce visual artifacts (pure reasoning, extraction, audit,
code review, etc.) do not need a render step.

### Composition principles for optimal skill design (Phase 3 gate)

Five principles discovered through the co-evolution of skills and MCP tools.
Apply these when designing the PDCA shape and step sequence:

#### 1. The Determinism Frontier

Every skill has a boundary between deterministic steps (output fully
determined by inputs) and probabilistic steps (LLM exercises judgment).
Push as much work as possible to the deterministic side.

- **`execute`** for MCP tool calls whose inputs are fully determined by prior
  step outputs (financial data, codegraph queries, calibration reads).
- **`compute`** (lisp.eval) for math, invariant checks, convergence signals.
- **`select`** only for steps that require LLM judgment (synthesis, reasoning,
  classification, prediction).

The test: "Could a deterministic function produce this output from these
inputs?" If yes, it should be `execute` or `compute`, not `select`.

Example: `company-research-flash` has 15 `execute` steps (deterministic
financial data + valuation tools) and 9 `select` steps (LLM synthesis).
The financial data is deterministic; the analysis is judgment.

#### 2. Persistence-Grounded Learning

Every skill that produces forecasts, analyses, or recommendations should
read its own prior outputs from MCP persistence before the cascade starts.
This closes the feedback loop: the skill's current invocation is informed
by its past performance.

The pattern: `execute` step at ordinal 0 → read prior runs → thread into
the first `select` step's `input_mapping`.

Examples:
- `superforecasting` reads `scenario_calibration` (Brier score history)
- `metacognition` reads `scenario_calibration` (overconfidence bias)
- `kata-improvement` reads `kanban_board_list` + `kanban_task_list` (prior PDCA cycles)
- `company-research-flash` reads `forecast_list` (prior price targets)
- `swarm-intelligence` reads `swarm_get_swarm` / `swarm_get_local_swarm` (prior swarm state)

#### 3. Failure Surfacing

Every `execute` step must declare `on_failure: { action: report, resume: "..." }`.
The `report` action calls `curator_report_skill_use_issue` before escalating
— the Curator receives the skill name, tool name, step ordinal, and error.
The `resume` text must explain what was lost and how to proceed without the
failed tool's output.

This is the enforcement point for the `.rules` trap: "Opt-in features that
fail must log the failure classification, not collapse to `None` via `.ok()?`."
Without `on_failure: report`, a failed `execute` step silently propagates as
`TemplateError` and the operator sees `ExitKind::Escalated` with no context.

#### 4. The Lisp Scaffold Pattern

When an LLM step produces structured output with invariant properties (count,
completeness, diversity, mutual exclusivity), follow it with a `lisp.eval`
`compute` step that checks those invariants deterministically. The Lisp step's
output (defect list or gap score) feeds the convergence signal.

Pattern: LLM generates → Lisp checks → LLM repairs (on next iteration).

Examples:
- `diagnose` step 4: Lisp checks hypothesis count (3-7), completeness
  (prediction + falsifier), diversity (≥2 likelihoods), mutual exclusivity
- `metacognition` steps 5-8: Lisp computes object gap, process gap,
  hypotenuse, Brier score
- `graph-audit` steps 19/21/22: Lisp computes convergence from quality
  findings / blockers count

The Lisp form is the deterministic scaffold; the LLM is the probabilistic
reasoner. The scaffold checks what the LLM cannot reliably self-evaluate.

#### 5. The Co-Evolution Loop

Skills and MCP tools evolve together. Skills reveal MCP tool design issues
(missing inputs, confusing schemas) via `on_failure: report`. The Curator
reads skill-use reports and issues `EvolveMcpToolSchema` directives. MCP
tools gain new capabilities that skills should adopt via `execute` steps.

When designing a new skill, check whether existing MCP tools provide
capabilities the skill should adopt natively (via `execute` steps) rather
than agent-mediated tool calls. The `skill-mcp-integration.md` reference
doc lists the two invocation patterns and when to use each.

### Phase 4 — Validate (delegate to skill-maintenance-validate)

Delegate to `skill-maintenance-validate` to check the scaffolded crate
against R1-R12, Z1-Z8, X1-X4, E1-E16.

### Phase 5 — Converge

Check that validation passed. If validation failed, identify the specific
failures and re-enter at Phase 1 (Research) with the failure report as
prior context — the failures may indicate that the ontological anchors
were insufficient or the PDCA shape didn't emerge correctly from them.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `create-skill-research.j2` | KnowAct | Search academic/industry literature for the skill's domain. Find ontological anchors (process ontologies, quality criteria, entity types). Derive the PDCA shape from the anchors. |
| `create-skill-describe.j2` | KnowAct | Capture the skill's purpose from the user's natural-language description. Choose a name (lowercase, hyphenated, 2-40 chars verb-noun or noun-noun, no reserved prefixes). Identify the PDCA phases the skill will have (most follow the bug-hunt pattern: a phased pipeline with feedback loop closure). Identify which skills this skill will compose (delegates). Identify the per-template span namespace (reg.<skill_name>.<phase>, ontology-derived); the ledger span_namespace is deterministic (reg.skill.<manifest.id>) and injected by the scaffold phase. |
| `create-skill-scaffold.j2` | KnowAct | Generate the full registry crate structure. Delegates to skill-maintenance-build for the mechanical scaffolding. Produces: (1) template manifest (kask/registry/templates/<name>/manifest.yaml) with crate metadata and template entries; (2) .j2 templates with [inference] frontmatter (template_type, contract visibility, generates_spans); (3) process manifest (kask/registry/manifests/<name>.yaml) with canonical actions gas/rjoule/convergence blocks, capability declarations; (4) SKILL.md companion (.agents/skills/<name>/SKILL.md) derived from the registry crate. |
| `create-skill-validate.j2` | KnowAct | Validate the scaffolded crate against all check categories. Delegates to skill-maintenance-validate (its check catalog is authoritative — this template mirrors it). Checks: R1-R12 (registry: manifest exists, crate metadata, templates non-empty, template entry valid, template_type, contract, visibility, .j2 exists, [inference] frontmatter, jinja2 body); Z1-Z8 (companion: SKILL.md exists, frontmatter, name matches directory, description present, no Magna Carta violations, headless, no deprecated markers, derived from registry); X1-X4 (cross-artifact: registry exists, no orphan SKILL.md, no drift, bidirectional name match); E1-E11 (executor compliance: canonical actions, gas block + cap adequate, rjoule block, convergence block, category valid, template_refs resolvable, sub-manifest budgets, step description, ordinal sequence, ledger.span_namespace == reg.skill.<manifest.id> with no abolished spans: list). Produces fix suggestions for any failures. |

## Constraints

- All flow templates are `KnowAct` type with `Public` visibility. Reference documents are `RenderAct`.
- rJoule cap: 3 per invocation. Maximum 10 iterations.
- **The skill's PDCA shape must emerge from its ontological anchors, not from a generic template.** The research phase is mandatory — no skill is scaffolded without ontological grounding.
- **Each skill is idiosyncratic.** Do not generalize the shape across skills. A gradient-hunter is not a bug-hunt is not a self-improvement cycle.
- The skill name must be lowercase, hyphenated, 2-40 characters, verb-noun or noun-noun, no reserved prefixes.
- The process manifest must use only canonical actions.
- The convergence block is mandatory for `category: skill` manifests. Use `convergence_mode: "cauchy"` with `cauchy_epsilon: 0.03`, `cauchy_window: 3`, `max_iterations: 10`, `min_iterations: 2`.
- The rjoule block is mandatory. rjoule.cap must be > 0 if any step uses `action: select`. The gas system is deprecated — do not add `gas:` blocks. Every step declares `timeout_seconds` as the runaway cutoff.
- The SKILL.md description must be ≤1024 bytes.
- **`lisp.eval` is available for custom deterministic compute steps.** When a skill needs a convergence formula, scoring function, or data transformation that doesn't fit the built-in `compute_ref`s (`kata.convergence_check`, `kata.object_gap`, etc.), use `compute_ref: lisp.eval` with an inline Lisp form. No Rust change needed — the manifest is the unit of authorship. See the manifest's comment block for an example. Security: gated to `category: skill` manifests only. The interpreter supports both prefix (`(+ a b)`) and infix (`a + b`) operator notation — use infix for simple scoring expressions (e.g., `score_a * 0.6 + score_b * 0.4`), prefix for complex nested logic with `let`, `if`, and `assoc`.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
