---
name: create-skill
description: "Create a new kask skill: SKILL.md process instructions + .j2 prompt templates. The SKILL.md is the process surface the agent reads and follows; templates are readable resources for prompt structure. The agent is the executor."
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
unreachable by `render_template`. Every skill follows this split —
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

**The `[inference]` header and the stage's D/P character (P8.4).** Every
template header is `[inference]`-keyed metadata terminated by a lone `---`
line, carrying the render contract (`contract.input` / `contract.output`)
that `render_template` validates before rendering. The header is present in
both regimes — it is the render contract, not the D/P marker. The stage's
D/P character is declared in the `{# goal: ... #}` comment: a pure render
(its rendered output is delivered as the step's result, no model call)
states "Deterministic (no LLM call)" — the
`sankey-flow/present-sankey.j2` pattern; a template whose rendered output
is a prompt a model consumes states its LLM-bearing role. The render
itself is D by construction in both cases (minijinja, no inference); the
goal comment tells the reader whether the *stage* is D or P.

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

Resolution state (live `onto_anchor`): Dublin Core is a published ladder
rung; PKO, GOLEM, MovieLabs OMC and SEPIO currently resolve only to the
core rung — derived-registry rulings pending. A generated skill anchoring
on a pending ontology accrues `research_findings` in Phase 4 until the
ruling lands; cite the ontology as the anchor source, and the ruling
closes the finding.

### How ontological anchoring shapes the skill

1. **PDCA shape**: the ontology's process structure implies the skill's
   phase structure. The SKILL.md's Instructions section follows this shape
   and MUST contain at least one PDCA (Plan→Do→Check→Act) self-improvement
   loop — the loop lives in the SKILL.md body; templates are leaves (steps)
   of the loop, rendered at the points the loop directs. Every loop carries a
   clear improvement dimension: the Check step's measurable signal, a
   threshold or convergence criterion, and a maximum iteration count (a
   stability condition may stop the loop earlier, but never replaces the
   maximum). The Check signal must be produced by a tool, a test or the
   operator — never by the model scoring its own output.
2. **Template contracts**: the ontology's entity types become the template's
   output fields. PKO's Procedure, Step, StepExecution become JSON fields.
3. **Tool selection**: the ontology's process determines which tools the
   SKILL.md instructs the agent to call at each phase.
4. **Convergence criteria**: the ontology's quality criteria become the
   `lisp_eval` convergence check the SKILL.md instructs the agent to run.

## Initial and target condition

- **Initial condition:** a description or a source skill, a Phase 0 discovery verdict that no installed skill already covers it, plus the ontological anchors found in Phase 1.
- **Target condition:** a SKILL.md and its templates with zero findings from the mechanical checks (prescreen, contract audit, S9/S10 sweep, corpus render tests) and zero unresolved S/T validation failures, plus one recorded functional trial in which a representative task run through the new skill meets its predeclared expected result. Structural validity alone is not the target: a skill can pass every check and still not do its job.

## Step types (D/P labelling)

| Phase | Type | Oracle / critique |
|-------|------|-------------------|
| 0 Discover | P + D | dimension scores are the model's (P, critiqued by the operator's extend-vs-create decision); fit, floor and band are recomputed in `lisp_eval` (D) |
| 1 Research, 2 Describe, 3 Scaffold | P | Phase 4 checks; the operator |
| Anchoring terms (Phase 1 step 3) | D | `onto_anchor` |
| 4 Validate — mechanical | D | audit scripts and `cargo test -p agent --lib corpus_` exit counts |
| 4 Validate — S1–S13 read | P | `skill-maintenance` validate, critiqued by the mechanical counts and the operator |
| 4 Functional trial | D when the predeclared result is checked by a tool or test; P when the operator judges it | the predeclared expected result for the trial task |
| 5 Converge | D | the `lisp_eval` gate over the Phase 4 counts; at most 2 re-entries |
| 3 Scaffold — pre-check scripts | D | `skill-corpus-prescreen.sh` and `skill-corpus-contract-audit.sh` exit counts; bound: two fix-and-rerun cycles |
| Translation — classify | P | Phase 4 checks; the operator |
| Translation — reconcile | D | the `lisp_eval` step-count form in the translation entry |

This skill's own provenance: the Phase 0–5 loop copies Deming's PDCA; the
artifact contract adapts `skill-maintenance`'s S1–S13/T1–T5 criteria;
Phase 0's routing adapts `skill-discovery`'s route phase; the
ontology-anchoring procedure and the teaching patterns are project
machinery (unanchored).

## PDCA Loop

The create-skill process itself follows a PDCA loop:

```
Plan:   Phase 0 — Discover     → Route against installed skills; extend or fold instead of creating when one fits
Plan:   Phase 1 — Research     → Find academic/industry ontological anchors
Plan:   Phase 2 — Describe     → Capture purpose, name, PDCA shape, delegates
Do:     Phase 3 — Scaffold    → Generate SKILL.md + .j2 templates
Check:  Phase 4 — Validate    → Count mechanical findings, S1–S13 failures, and run one functional trial
Check:  Phase 5 — Converge     → lisp_eval gate over the counts
Act:    Phase 5 — Converge     → Re-enter at the phase that owns the failure — anchors first (1: missing anchors; artifact work on missing anchors is wasted, so scaffold findings wait for the next pass), else 3 (artifact/trial defects); at most 2 re-entries
```

## When NOT to Use

- Validating an existing skill — use `skill-maintenance` (this skill's Phase 4 delegates there anyway).
- Matching tasks to installed skills — use `skill-discovery` (route); this skill's Phase 0 runs that route before creating anything.
- Extending an existing skill that Phase 0 found covers the need — use `skill-maintenance`'s optimize loop.
- Auditing template/manifest logic — use `skill-maintenance`'s template-logic audit (SKILL.md bodies go through its optimize loop).

## Instructions

### Phase 0 — Discover (does an installed skill already cover this?)

Run once, before any research or writing. A new skill that duplicates or belongs inside an existing one is the failure this gate prevents (observed: adapter-lifecycle was authored standalone and later folded into `self-improvement`, 2026-09-26).

1. Call the `skill` tool with name `skill-discovery` and the proposed skill's purpose as `task`; run its route phase against the installed catalog.
2. Recompute the best fit and coverage band with route's `lisp_eval` form, from its reported dimension scores.
3. Act on the band:
   - `full` (best fit ≥ 0.80): stop. Report the covering skill; the request is routed there, not built. Creating anyway requires the operator's explicit override, recorded with the reason.
   - `partial` (0.40–0.79): run discovery's detect-gap phase on the uncovered capabilities. `extend_skill` or `route_to_existing_skill` → present to the operator the choice between extending (or folding into) that skill and creating a new one, with the fit evidence; extension goes through `skill-maintenance`'s optimize loop, not this skill. Continue to Phase 1 only on a `create_skill` recommendation or the operator's choice to create.
   - `none` (< 0.40): continue to Phase 1, carrying `uncovered_capabilities` as the scope.
4. Record the verdict (band, best-fitting skill and fit, decision) in the Phase 2 specification. This gate runs once; Phase 5 re-entries never return here. The dimension scores are the model's judgment; only the arithmetic is deterministic, so a borderline band (within 0.05 of 0.40 or 0.80) is presented to the operator rather than acted on automatically. The route form is owned by `skill-discovery` — this skill recomputes from its reported dimension scores under the bands above; if the reported band and the recomputed band disagree, treat the case as borderline and present both to the operator rather than acting automatically.

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
3. Call `onto_anchor` for each domain term the skill will name, classify or compute (its process, entity types and quality criteria). Record each resolved anchor and its rung; a coarse (core-rung) anchor carries its ruling path, never a private definition. Count the terms that resolved only to the core rung.
4. Select the ontology reference set.
5. Derive the PDCA shape from the anchors.
6. Write the functional trial now, before any scaffolding: one representative task for the new skill and its expected result, stated so a tool, test or the operator can check it (e.g. a deterministic contains/regex evaluator, an expected output field, or a named operator judgment). The trial is fixed before the artifact exists so the artifact cannot be fitted to it.

### Phase 2 — Describe (capture specification)

1. Capture the skill's purpose from the user's natural-language
   description, informed by the research phase.
2. Choose a name: lowercase, hyphenated, 2-40 characters, verb-noun or
   noun-noun, no reserved prefixes.
3. Specify the PDCA phases — these emerge from the research phase, not
   from a generic template. Each phase is grounded in an ontological anchor.
4. Identify which skills this skill will compose (delegates via `skill` tool). If it runs 3+ peer skills on one task and needs their outputs merged, compose `skill-bundler` for the merge rather than writing a merge step.
5. Identify which MCP tools the skill will call (e.g., `curator_memory_recall`,
   `curator_consult`, `kanban_task_list`, `stock_quote`, `web_search`).
6. Identify which agent tools the skill will call (e.g., `lisp_eval` for
   deterministic computation, `read_file` for template loading).
7. Select the teaching patterns the new skill instantiates — convergence,
   composition, persistence-grounded learning, failure surfacing, D/P
   labelling (the patterns section below) — and record them in the
   specification; Phase 3's scaffold carries them into the generated
   artifacts.

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

2. **.j2 templates** in `kask/registry/templates/<name>/`, one per reasoning
   phase. Render `create-skill/scaffold` with `skill_description` and
   `scope` to produce both artifacts. Every template must meet the artifact
   contract that `skill-maintenance-validate` (T2–T5) and the corpus tests
   enforce:
   - **First line `{# goal: ... #}`, copied verbatim from that template's row
     in the generated SKILL.md's Registry Templates table** — the row IS the
     goal, so the two cannot drift. One goal block per template: a long goal
     is one long line, never several consecutive `{# ... #}` blocks (a
     wrapped goal parses only partly).
   - An `[inference]` contract header (typed `input`/`output` fields only)
     terminated by a lone `---` line. No `visibility` key and no body
     `[inference]` parameter stanza: `render_template` reads only
     `contract.input` (crates/agent/src/tools/render_template_tool.rs
     `validate_contract_inputs`) and strips everything else — any other
     header key is decoration.
   - Jinja2 variables matching the contract inputs, and only those; every
     declared output named in the output instructions.
   - The expected JSON output shape.
3. **Pre-check before handing over:** write the templates, then run
   `bash kask/scripts/audit/skill-corpus-prescreen.sh` and
   `bash kask/scripts/audit/skill-corpus-contract-audit.sh` via `terminal`.
   A new template that either script flags is a scaffold defect: fix it and
   re-run, at most twice, before Phase 4. The scaffold and translate
   templates do not report validation; these scripts are the check.

### From a source skill (translation entry)

When the starting point is a skill from another agent system rather than a
description, replace Phases 2–3 with translation: render
`create-skill/translate` with the classified `source_skill` and
`target_domain`. Map source steps to numbered SKILL.md instructions and source
tool calls to kask tools (deterministic computation → `lisp_eval`, data
retrieval → the MCP tool, composition → `skill`, prompts →
`render_template`, files → `read_file`/`write_file`/`edit_file`). Mark any
source concept with no kask equivalent `[unresolved: no kask equivalent for
<source_ref>]` instead of inventing one. Reconcile the returned
`translation_summary` with `lisp_eval`: form
`(= source_steps (+ steps_mapped (length unresolved_concepts)))`; a false
result means a source step was silently dropped — re-render before Phase 4.
The artifact contract and pre-check above apply unchanged; then continue at
Phase 4.

#### How to write SKILL.md instructions that use tools

Each instruction step should be concrete and tool-oriented:

```
### Step: Analyze (example)

1. Call `render_template` to render the analysis template:
   template: my-skill/analyze (resolves to kask/registry/templates/my-skill/analyze.j2)
   variables: { "target": "{{ target }}", "prior_results": <step 2 output> }

2. Following the template's output schema, analyze the research from
   step 2. Produce a JSON object with the fields specified in the template.

3. Call `lisp_eval` to check structural invariants:
   form: "(let ((results (assoc \"findings\" analysis))) (length results))"
   env: { "analysis": <your analysis output> }
   If the result is 0, return to step 2 and produce more findings.

4. Call `curator_consult` to check prior analyses:
   query: "prior {{ skill_name }} analyses for {{ target }}"
   Thread the relevant memories into your next analysis step.
```

#### Convergence pattern

The SKILL.md describes when to loop in natural language, backed by
`lisp_eval` for deterministic checks:

```
### Convergence

After each iteration, count what an external oracle still reports open —
failing tests, compiler errors, unresolved tool findings — and check it in
`lisp_eval`:
  form: "(if (= open 0) (quote done) (if (>= iteration 3) (quote stop-and-report) (quote iterate)))"
  env: { "open": <count from the tool output>, "iteration": <1-based> }

`done` proceeds to the report; `iterate` re-enters the named step with the
open items; `stop-and-report` delivers what exists with the open items
listed. The count comes from the tool, never from the model's own score.
```

#### Composition pattern

The SKILL.md instructs the agent to call the `skill` tool to compose
with another skill:

```
### Step: Delegate validation (example)

Call the `skill` tool:
  name: "skill-maintenance"
  task: "validate skill {{ skill_name }} against the SKILL.md quality checks"
```

#### Persistence-grounded learning pattern

The SKILL.md instructs the agent to call an MCP tool for prior context:

```
### Step: Prior context (example)

Before starting, call `curator_memory_recall`:
  entity: "{{ target }}"
  This retrieves prior analyses of this target. Thread relevant
  findings into your initial analysis.
```

#### Failure surfacing pattern

The SKILL.md instructs the agent to call `curator_report_skill_use_issue`
on tool failures:

```
If any MCP tool call fails, call `curator_report_skill_use_issue` with:
  skill_name: "my-skill", tool_name: <failed tool>, error: <error message>
Then continue with the best available information — do not abort.
```

#### D/P labelling pattern

Every computation-prescribing SKILL.md carries a **D/P labelling** section —
the convention ratified as P8.4 (entropy-matched computation). For each step
that prescribes computation:

```
### Step: Route the computation (example)

Label each step D or P per P8.4:
- **D** — name the oracle (`lisp_eval`, `lean_check`, `cargo`, a server-side
  tool). The answer is pinned and verification is cheap.
- **P** — name the collapse path: propose-verify (P step, D gate), explicit
  probabilistic compute (D server over P inputs), or calibrated forecast
  (external ground truth). Name the critique path (the operator, a separate
  render, resolved outcomes).
```

Hybrid steps label at sub-step granularity; `render_template` is D
(deterministic render) feeding P (model consumption). The section is the
audit floor — presence is checkable by filesystem walk; substance (does each
D step name a real oracle, does any P step hide a cheap deterministic check)
is judged in review. A skill that calls `lisp_eval` and labels itself "all P"
is checkably wrong.

### Phase 4 — Validate (mechanical counts, S1–S13, functional trial)

1. **Mechanical checks (D).** Run via `terminal` and count the findings
   that name the new skill or its templates:
   - `bash kask/scripts/audit/skill-corpus-prescreen.sh`
   - `bash kask/scripts/audit/skill-corpus-contract-audit.sh`
   - `bash kask/scripts/audit/skill-corpus-callsite-audit.sh`
   - `bash kask/scripts/audit/skill-corpus-s9-s10-sweep.sh`
   - `cargo test -p agent --lib corpus_` (template render and metadata strip; a failure counts once per failing test)
   Count from the full output (`grep -c` on the saved output), never from a truncated display. The sum is `scaffold_findings`.
2. **S1–S13 read (P).** Call the `skill` tool with name
   `skill-maintenance` and task "validate skill <name>", and work its
   validate phase. The `skill` tool returns instructions only: you perform
   the checks. Add each failed S/T check to `scaffold_findings`. A check you
   could not perform is reported `unverified`, never passed.
3. **Functional trial (D).** Run the trial task fixed in Phase 1 step 6
   through the new skill (follow its SKILL.md, or delegate it via
   `swarm_delegate_local` to a card declaring the skill), and check the
   result against the expected result with the named evaluator
   (`swarm_evaluate_local` for contains/not_contains/regex). Set
   `functional` to 1 if it met the expectation, else 0. If the trial cannot
   run (a required tool, key or spend is unavailable), set `functional` to 0
   and record the blocker; the skill is not done.
4. Set `research_findings` to the number of Phase 5-relevant research gaps:
   PDCA phases with no cited anchor, plus required domain terms that
   `onto_anchor` could not resolve beyond the core rung without a ruling.

### Phase 5 — Converge

Call `lisp_eval` with the Phase 4 counts and the number of re-entries so far:
  form: "(cond ((and (= scaffold 0) (= research 0) (= functional 1)) (quote done)) ((>= reentries 2) (quote stop-and-report)) ((> research 0) (quote reenter-phase-1)) (t (quote reenter-phase-3)))"
  env: { "scaffold": <scaffold_findings>, "research": <research_findings>, "functional": <0 or 1>, "reentries": <0-based count> }

- `done`: report the skill with its counts and the trial evidence.
- `reenter-phase-3`: fix the artifacts against the listed findings or the
  trial failure, then rerun Phase 4. Do not redo research for an artifact
  defect.
- `reenter-phase-1`: research the missing anchors, then continue through
  Phases 2–4.
- `stop-and-report`: deliver what exists with every open finding and the
  trial result listed; the operator decides.

The trial task and its expected result never change between re-entries;
changing them to pass is fitting the test to the artifact.

Every scaffolded SKILL.md states: an initial condition (inputs and how the
current state is measured), an observable target condition, its bounded PDCA
loop (or a justified single-pass exemption), and a step-type table labelling
each step D (naming its oracle) or P (naming its critic and collapse path),
per the D/P labelling pattern (P8.4).

## Regression case

Render `create-skill/scaffold` with a one-line `skill_description` (e.g. "a chunked-corpus QA skill") and `scope`; render `create-skill/translate` with a small `source_skill` and `target_domain`. Gates: the Phase 5 convergence form over four envs — all-clean `{scaffold 0, research 0, functional 1, reentries 0}` → `done`; exhausted `{2, 0, 0, 2}` → `stop-and-report`; anchors-first `{1, 1, 0, 0}` → `reenter-phase-1`; artifact-only `{1, 0, 0, 0}` → `reenter-phase-3`. The translation reconciliation form over a 3-step source with 1 unresolved concept → `true`. Generated-artifact check (the recorded trial shape for this generative skill): the rendered scaffold's artifact lists carry the same contract markers as Phase 3 step 2 (goal-first line verbatim from the table, one goal block, typed `[inference]` header only, no visibility key, no body stanza) — a divergence between the two texts is a finding. Both forms' receipts above are executed live through `lisp_eval`.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `scaffold.j2` | Scaffold a complete skill (SKILL.md body carrying the PDCA loop + .j2 step-leaf templates) from a description, with goals stamped from the Registry Templates rows and typed `[inference]` contracts; validation is left to the Phase 3 scripts and Phase 4. |
| `translate.j2` | Translate a classified source skill from another agent system into a SKILL.md body plus .j2 templates under the same artifact contract, marking concepts with no kask equivalent as unresolved. |

To render a template, call `render_template` with the ref (e.g. `create-skill/scaffold`):
- `scaffold.j2`: `skill_description`, `scope`
- `translate.j2`: `source_skill`, `target_domain`

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
