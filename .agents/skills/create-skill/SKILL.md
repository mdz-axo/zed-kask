---
name: create-skill
core: true
description: "Create a new kask skill: SKILL.md process instructions + .j2 prompt templates. The SKILL.md is the process surface the agent reads and follows; templates are readable resources for prompt structure. The agent is the executor."
steps:
  - id: prior_context
    tools:
      - curator_memory_recall
  - id: design
    tools:
      - render_template
  - id: validate
    tools:
      - lisp_eval
---

# Create Skill

Create a new kask skill as a SKILL.md process document with companion .j2
prompt templates. The agent reads the SKILL.md, follows its instructions,
and calls tools (`lisp_eval`, MCP tools, `read_file`, `skill`, etc.) as
directed. The agent IS the executor.

## The skill model

A kask skill has two artifacts, in **two different locations**:

```
.agents/skills/<name>/
└── SKILL.md              # Process instructions the agent reads and follows

kask/registry/templates/<name>/
└── <phase>.j2            # Jinja2 prompt templates (render_template resources)
```

Templates do NOT live next to the SKILL.md. The `render_template` tool
resolves template refs against the registry base path
(`kask/registry/templates/`, wired via `agent::set_template_base_path()` in
`crates/zed/src/main.rs`); a template placed in `.agents/skills/<name>/` is
unreachable by `render_template`. Every shipped skill follows this split —
zero `.j2` files exist under `.agents/skills/`.

### SKILL.md — the process surface

The SKILL.md is the primary artifact. The `skill` tool reads its body and
returns it to the agent as instructions. The agent then follows those
instructions, calling tools at each step.

The SKILL.md contains:

1. **Frontmatter**: `name`, `description`, `core` (optional). No `visibility`.
2. **When to Use / When NOT to Use**: triggers and anti-triggers.
3. **Instructions**: numbered steps the agent follows. Each step says what
   tool to call, what inputs to provide, and what to do with the result.
4. **Constraints**: guardrails, budgets, rules.

### .j2 templates — prompt resources

Templates are `.j2` files. There are two ways to use them:

1. **`render_template`** — the built-in rendering tool. Pass the template
   ref as `<skill-name>/<file>` (e.g. `my-skill/analyze`; the resolver
   tries the `.j2` extension) and a `variables` map. The tool renders the
   Jinja2 template with minijinja from `kask/registry/templates/<name>/`
   and returns the rendered string. Use this when the template has Jinja2
   variables that should be substituted from prior step results.

2. **`read_file`** — when the template is a *prompt specification* the agent
   reads to understand an expected output shape, not a template to render.
   Read it from `kask/registry/templates/<name>/<file>.j2`. The agent reads
   it, internalizes the structure, and produces output
   following that guidance.

The template defines:

- The prompt structure (system prompt, output schema)
- Jinja2 variables that the agent fills from prior step results
- The expected JSON output shape

## Core principle: idiosyncratic, not generalized

Each skill is customized and idiosyncratic to its domain. The skill's
PDCA shape emerges from its ontological anchors — the academic and
industry processes that define how the domain works. A gradient-hunter
follows Prior → Map → Detect → Hypothesize → Report → Convergence because
that's what gradient analysis IS. A bug-hunt follows Charter → Probe →
Oracle → Taxonomize → Report → Convergence because that's what exploratory
testing IS.

**Do not generalize the shape.** Copying the bug-hunt pattern for a
non-bug-hunt skill produces a hollow skill — the shape without the
anchors. The create-skill process finds the anchors first, then lets the
shape emerge.

## When to Use

- You need to create a new kask skill from a natural-language description.
- You need to scaffold the SKILL.md + template structure with ontological
  grounding.

Do NOT use for:
- Validating an existing skill (use `skill-maintenance`).
- Auditing skill health (use `skill-maintenance`).

## Ontological anchoring

Every skill is grounded in ontological anchors — the academic and industry
processes that define how the skill's domain works. The create-skill
process finds these anchors in the research phase and embeds them in the
skill's artifacts.

### Ontology reference set

| Ontology | Domain | Use when |
|---|---|---|
| **PKO** (Procedural Knowledge Ontology) | Industrial processes, procedures | The skill models a procedure with specification/execution separation |
| **Dublin Core** | Metadata, documentation | The skill produces or manages metadata artifacts |
| **GOLEM** | Narrative, fiction, storytelling | The skill models narrative structure |
| **MovieLabs OMC** (Ontology for Media Creation) | Media production workflows | The skill models media creation pipelines (capture → post → distribution) |
| **SEPIO** (Scientific Evidence and Provenance Information Ontology) | Evidence, provenance, scientific claims | The skill models evidence lines and assertions |
| **Domain-specific** | Any | The research phase finds ontologies specific to the skill's domain |

### How ontological anchoring shapes the skill

1. **PDCA shape**: the ontology's process structure implies the skill's
   phase structure. The SKILL.md's Instructions section follows this shape.
2. **Template contracts**: the ontology's entity types become the template's
   output fields. PKO's Procedure, Step, StepExecution become JSON fields.
3. **Tool selection**: the ontology's process determines which tools the
   SKILL.md instructs the agent to call at each phase.
4. **Convergence criteria**: the ontology's quality criteria become the
   `lisp_eval` convergence check the SKILL.md instructs the agent to run.

## PDCA Loop

The create-skill process itself follows a PDCA loop:

```
Plan:   Phase 1 — Research     → Find academic/industry ontological anchors
Plan:   Phase 2 — Describe     → Capture purpose, name, PDCA shape, delegates
Do:     Phase 3 — Scaffold    → Generate SKILL.md + .j2 templates
Check:  Phase 4 — Validate    → Run skill-maintenance validation
Check:  Phase 5 — Converge     → Check validation passed; if not, re-enter
Act:    Phase 6 — Loop        → If validation failed, re-enter at Phase 1
```

## Composed Skills

| Skill | Role | When Invoked |
|-------|------|-------------|
| `skill-maintenance` | Validation | Phase 4 (validate) |

## Instructions

### Phase 1 — Research (find ontological anchors)

1. Search the academic and industry literature for the skill's domain.
   Use `web_search` and academic search tools to find:
   - **Process ontologies**: how does the domain's process work? What
     are the canonical phases, steps, or stages?
   - **Quality criteria**: how does the domain measure success? What
     are the convergence criteria?
   - **Entity types**: what are the domain's objects, events, roles?
   - **Existing ontologies**: is there a PKO, Dublin Core, GOLEM, MovieLabs OMC, SEPIO,
     or domain-specific ontology that formalizes this domain?
2. Record the ontological anchors: for each anchor, cite the source
   (author, year, paper/standard) and describe how it shapes the skill.
3. Select the ontology reference set.
4. Derive the PDCA shape from the anchors.

### Phase 2 — Describe (capture specification)

1. Capture the skill's purpose from the user's natural-language
   description, informed by the research phase.
2. Choose a name: lowercase, hyphenated, 2-40 characters, verb-noun or
   noun-noun, no reserved prefixes.
3. Specify the PDCA phases — these emerge from the research phase, not
   from a generic template. Each phase is grounded in an ontological anchor.
4. Identify which skills this skill will compose (delegates via `skill` tool).
5. Identify which MCP tools the skill will call (e.g., `curator_memory_recall`,
   `curator_consult`, `kanban_task_list`, `stock_quote`, `web_search`).
6. Identify which agent tools the skill will call (e.g., `lisp_eval` for
   deterministic computation, `read_file` for template loading).

### Phase 3 — Scaffold (generate SKILL.md + templates)

Generate the skill artifacts:

1. **SKILL.md** with:
   - Frontmatter: `name`, `description`
   - "When to Use" / "When NOT to Use" sections
   - "Instructions" section with numbered steps. Each step specifies:
     - What tool to call (`lisp_eval`, MCP tool, `read_file`, `skill`)
     - What inputs to provide (form, env, query, template path, etc.)
     - What to do with the result (feed to next step, check convergence)
   - "Constraints" section with guardrails and rules
   - Template references: "Render `my-skill/analyze` (or read
     `kask/registry/templates/my-skill/analyze.j2`) for the expected output
     format"

2. **.j2 templates** in `kask/registry/templates/<name>/` with:
   - A comment header describing the template's purpose and phase
   - Jinja2 variables for context injection (`{{ task }}`, `{{ step_1_result }}`)
   - The prompt structure (what the agent should analyze/synthesize)
   - The expected JSON output shape (as a comment or schema description)

### How to write SKILL.md instructions that use tools

Each instruction step should be concrete and tool-oriented:

```
### Phase 3 — Analyze

1. Call `render_template` to render the analysis template:
   template: my-skill/analyze (resolves to kask/registry/templates/my-skill/analyze.j2)
   variables: { "target": "{{ target }}", "prior_results": <step 2 output> }

2. Following the template's output schema, analyze the research from
   step 2. Produce a JSON object with the fields specified in the template.

3. Call `lisp_eval` to check structural invariants:
   form: "(let ((results (assoc \"findings\" step_3_result))) (length results))"
   env: { "step_3_result": <your analysis output> }
   If the result is 0, return to step 2 and produce more findings.

4. Call `curator_consult` to check prior analyses:
   query: "prior {{ skill_name }} analyses for {{ target }}"
   Thread the relevant memories into your next analysis step.
```

### Convergence pattern

The SKILL.md describes when to loop in natural language, backed by
`lisp_eval` for deterministic checks:

```
### Convergence

After each analysis iteration, call `lisp_eval` to compute the convergence
signal:
  form: "(+ (assoc \"confirmed\" step_N_result) (assoc \"potential\" step_N_result))"
  env: { "step_N_result": <latest analysis output> }

If the signal is 0 (no open findings), the analysis is complete — proceed
to the report. If the signal decreased by less than 20% from the prior
iteration, stop and report what you have (diminishing returns).
```

### Composition pattern

The SKILL.md instructs the agent to call the `skill` tool to compose
with another skill:

```
### Phase 4 — Delegate validation

Call the `skill` tool:
  name: "skill-maintenance"
  task: "validate skill {{ skill_name }} against the SKILL.md quality checks"
```

### Persistence-grounded learning pattern

The SKILL.md instructs the agent to call an MCP tool for prior context:

```
### Phase 0 — Prior context

Before starting, call `curator_memory_recall`:
  entity: "{{ target }}"
  This retrieves prior analyses of this target. Thread relevant
  findings into your initial analysis.
```

### Failure surfacing pattern

The SKILL.md instructs the agent to call `curator_report_skill_use_issue`
on tool failures:

```
If any MCP tool call fails, call `curator_report_skill_use_issue` with:
  skill_name: "my-skill", tool_name: <failed tool>, error: <error message>
Then continue with the best available information — do not abort.
```

### Phase 4 — Validate (delegate to skill-maintenance)

Call the `skill` tool:
  name: "skill-maintenance"
  task: "validate skill {{ skill_name }}"

### Phase 5 — Converge

Check that validation passed. If validation failed, identify the specific
failures and re-enter at Phase 1 with the failure report as prior context.

## Constraints

- The SKILL.md is the process surface — the agent reads it and follows it.
  Write instructions as imperative steps the agent can execute.
- .j2 templates are readable resources, not executed code. They define
  prompt structure and expected output shape.
- Use `lisp_eval` for all deterministic computation (counting, scoring,
  invariant checks, convergence signals).
- Use MCP tools directly for data retrieval, persistence, and actions.
- Use the `skill` tool to compose with other skills.
- Name must be lowercase, hyphenated, 2-40 characters, verb-noun or
  noun-noun, no reserved prefixes.
- Core skills (`core: true` in frontmatter) are always-on, re-seeded on
  every startup, and cannot be shadowed by project-local skills of the
  same name. Only names in `CORE_SKILL_NAMES` may declare `core: true`.
