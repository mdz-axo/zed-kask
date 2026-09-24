---
name: skill-maintenance
core: true
description: "Skill lifecycle management: validate canonical SKILL.md instructions and companion .j2 templates, audit staleness and coverage, and compare distinct skill designs on fixed tasks before claiming improvement."
---

# Skill Maintenance

Skill lifecycle management and maintenance. SKILL.md is the canonical source
of truth — the process instructions the agent reads and follows. .j2 templates
are companion resources that define prompt structure. Audit staleness,
coverage gaps, and task outcomes. Validate, build, translate, and compare
candidate designs before changing skills.

## The skill model

A kask skill has two artifacts, in **two different locations**:

```
.agents/skills/<name>/
└── SKILL.md              # Process instructions (source of truth)

kask/registry/templates/<name>/
└── <phase>.j2            # Prompt templates (companion resources)
```

Templates do NOT live next to the SKILL.md — `render_template` resolves
refs against the registry base path (`kask/registry/templates/`), so a
template placed in `.agents/skills/<name>/` is unreachable (see check T5).

The agent reads the SKILL.md, follows its instructions, and calls tools
(`lisp_eval`, MCP tools, `read_file`, `skill`) as directed.

## When to Use

- When you need to validate a skill's SKILL.md structure and template quality.
- When you need to scaffold a new skill (SKILL.md + templates) from a
  natural-language description.
- When you need to translate a classified source skill into the kask format.
- When you need to audit the canonical SKILL.md and referenced templates for staleness signals and health scoring.
- When you need to map task patterns against the skill corpus for coverage gaps.
- When you need to improve a skill's actual task performance, not merely its structural health score.

## When NOT to Use

- Auditing `.j2` template or `manifest.yaml` logic — use `skill-logic-audit` (its target class; SKILL.md bodies are not valid logic-audit targets).
- Authoring a new skill from scratch — use `create-skill` (it delegates validation back here at Phase 4).
- Matching tasks to installed skills — use `skill-router`; acquiring new ones — `skill-discovery`.

## Instructions

### skill-maintenance-validate

1. Validate the specified skill or all skills in `.agents/skills/` against:
   - **S1**: SKILL.md exists in `.agents/skills/<name>/SKILL.md`
   - **S2**: SKILL.md frontmatter has `name` and `description` fields
   - **S3**: SKILL.md `name` matches the directory name
   - **S4**: SKILL.md `description` is present and non-empty (1-500 chars)
   - **S5**: SKILL.md has a "When to Use" section
   - **S6**: SKILL.md has an "Instructions" section with numbered steps
   - **S7**: SKILL.md instructions reference concrete tools (`lisp_eval`,
     MCP tools, `read_file`, `render_template`, `skill`) — not abstract "the system will" language
   - **S8**: SKILL.md has a "Constraints" section
   - **S9**: No `visibility` field in frontmatter. Mechanical enforcement
     point: `kask/scripts/audit/skill-corpus-s9-s10-sweep.sh` (frontmatter
     dispatch-key sweep).
   - **S10**: SKILL.md does not use removed vocabulary (`compute_ref`,
     `action:`, `template_ref` as a manifest dispatch key, `convergence_signal`,
     `input_mapping`, `on_failure`, `ordinal:`) or vestigial `steps` frontmatter with
     `id`/`tools` dispatch structure (manifest-executor remnant). The
     `render_template` tool's `template_ref` parameter, named in call
     instructions, is the live contract — not a violation. Mechanical
     enforcement point: `kask/scripts/audit/skill-corpus-s9-s10-sweep.sh`
     (frontmatter dispatch keys + body key-form sweep of the no-live-contract
     tokens; `template_ref`/`action` body usages adjudicate at read-triage
     under this check).
   - **S11**: If `core: true` is declared, the name must be in
     `CORE_SKILL_NAMES` (enforced by `agent_skills` at load time)
   - **S12**: Every `lisp_eval` form pinned in a SKILL.md's instructions
     evaluates against the interpreter's builtin surface. Method:
     extract each pinned form; run it via `lisp_eval` with a stub env
     (empty list for list-shaped variables, 0 for numeric ones); an
     `unbound symbol: X` error where X is not one of your stub variables
     is a broken form (missing builtin or special form). Type or runtime
     errors over stub values are fine — the form's symbols resolved.
   - **S13**: The SKILL.md body contains at least one PDCA
     (Plan→Do→Check→Act) self-improvement loop as the skill's core
     process. The loop lives in the SKILL.md body — templates are
     leaves (steps) of the loop, never its carrier. The loop must
     carry a clear improvement dimension: the Check step's measurable
     signal, a threshold or convergence criterion, and a bound (max
     iterations or a stability condition). A body with phases but no
     Check→Act feedback path, or a Check with no named signal, fails
     this check.
     Loop vocabulary recognition (auditor guidance): the loop may be
     carried in any of these forms — (a) explicit PDCA phases; (b) a
     convergence gate (lisp_eval or otherwise) plus a named re-entry
     point; (c) bounded rounds or escalation (max rounds, max
     attempts, escalate-after-N); (d) cycle vocabulary
     (red-green-refactor with routing re-entry, zero-delta abort,
     per-turn gate with a revision bound, elimination-to-survivor with
     a materiality guard). In every form the auditor requires the four
     anatomy parts: a named Check signal, a threshold or convergence
     criterion, a bound (max iterations or a stability/abort
     condition), and an Act re-entry (the phase that re-enters, or an
     explicit terminal action: escalate/halt/report). A loop whose
     Check→Act lives only in a template's purpose text fails — the
     loop lives in the body (composition law).
     Exempt classes (operator ratification 2026-09-09, DR-S13a): role
     guides (product-manager), documented single-pass skills
     (skill-bundler, sankey-flow, prompt-enhance, swarm-steering), and
     doc-style handbooks (gpui-bench) are
     exempt. An exempt body must carry a one-line marker naming the
     exemption so future audits do not re-litigate the design.
   - **T1**: Each `.j2` template referenced in SKILL.md instructions exists
     in the skill's registry template crate
     (`kask/registry/templates/<name>/`)
   - **T2**: Each `.j2` template carries a `{# goal: ... #}` annotation
     describing its purpose — the exact format skill-logic-audit's
     logic-load-goal step parses; a comment header in any other form is a
     fail (an unparseable goal is unauditable). Mechanical enforcement
     point: `kask/scripts/audit/skill-corpus-prescreen.sh` (goal presence,
     length, and content-overlap pre-screen over the whole corpus; flagged
     templates get read-triage under this check).
   - **T3**: Each `.j2` template defines expected output fields (as comments
     or schema description)
   - **T4**: `[inference]` blocks in .j2 templates follow the two-stanza
     convention — templates ARE inference prompts, so the marker is the
     metadata carrier, not a defect. The rule is about placement:
     (a) the header `[inference]` block (contract + visibility) must be
     terminated by a lone `---` line; (b) at most one body `[inference]`
     param stanza (temperature/work_effort/verbosity/thinking_budget),
     placed at the top of the body. `render_template` strips both; a
     third `[inference]` block or a header missing its `---` terminator
     is a fail. Mechanically enforced corpus-wide by
     `corpus_templates_strip_without_leaking_metadata`
     (crates/agent/src/tools/render_template_tool.rs).
   - **T5**: If a template is referenced for rendering via `render_template`,
     it is reachable from the `render_template` base path (registry templates
     directory, not the skill directory). Mechanically enforced corpus-wide
     by `corpus_templates_render_without_error` and
     `corpus_includes_resolve_and_strip`
     (crates/agent/src/tools/render_template_tool.rs).
2. Evaluate every check for every targeted skill without omissions.
3. Include specific evidence for any fail results (file path, line number).
4. Provide actionable fix suggestions for any failures.
5. Respond with a JSON object containing validation results and fix suggestions.

### skill-maintenance-build

1. Generate a complete skill (SKILL.md + .j2 templates) from the user's
   natural-language description.
2. Ensure the skill name is lowercase, hyphenated, 2-40 characters,
   verb-noun or noun-noun, and lacks reserved prefixes.
3. Create the SKILL.md with:
   - Frontmatter: `name`, `description`
   - "When to Use" / "When NOT to Use" sections
   - "Instructions" section with numbered, tool-oriented steps
   - "Constraints" section
4. Create .j2 templates for each reasoning phase:
   - `{# goal: ... #}` annotation as the first line, derived verbatim from
     the Registry Templates row this build writes into the generated
     SKILL.md — the row IS the goal (single source, no drift between the
     table and the annotation). One `{# goal: ... #}` block per template:
     a long goal is one long line, never consecutive `{# ... #}` blocks
     (wrapped goals parse partially under logic-load-goal and the
     prescreen).
   - `[inference]` contract header (input/output fields, `visibility`),
     terminated by a lone `---` line
   - Jinja2 variables for context, matching the contract inputs
   - Expected JSON output shape
5. Run the generated templates through
   `kask/scripts/audit/skill-corpus-prescreen.sh` before responding — every
   generated template must pass (goal presence, length, overlap). A
   generated template that fails the prescreen is a build defect, not a
   triage candidate.
6. Derive the PDCA shape from the skill's ontological anchors (see create-skill).
7. Respond with the SKILL.md content, template contents, and validation status.

### skill-maintenance-translate

1. Convert a classified source skill (e.g., from another agent system) into
   the kask format: SKILL.md + .j2 templates.
2. Map source process steps to SKILL.md instruction steps.
3. Map source tool calls to kask tools:
   - Deterministic computation → `lisp_eval`
   - Data retrieval → appropriate MCP tool
   - Skill composition → `skill` tool
   - Prompt rendering → `render_template`
   - File operations → `read_file`, `write_file`, `edit_file``
4. Create .j2 templates for reasoning steps that need structured prompts.
5. Mark any source concepts with no kask equivalent as
   `[unresolved: no kask equivalent for <source_ref>]`.
6. Respond with the SKILL.md, templates, and a translation summary.

### skill-maintenance-audit

1. Read `.agents/skills/<name>/SKILL.md` as the canonical process; if it is missing, report that as a critical loss of the skill. Inspect only the `.j2` templates the body references under `kask/registry/templates/<name>/`. Manifests do not dispatch skills and must not supply health penalties, retirements, or a substitute for a missing SKILL.md.
2. Confirm each signal against the actual tree and live tool surface, citing file:line (or the expected path and directory listing for a missing file): missing SKILL.md; removed/nonexistent tool references; referenced templates that are missing or unreachable; removed manifest-dispatch vocabulary used as instructions (not historical quotations or live `render_template` parameters); missing Constraints; vague instructions with no actionable tool steps; malformed `[inference]` headers or more than two `[inference]` blocks; or a concrete contradiction with the runtime or project constraints. Do not penalize speculative or unverified claims. Keep real broken references and malformed templates as findings.
3. Score each **distinct verified defect** once from 1.0, floor at 0.0: critical −0.50 (missing SKILL.md, nonexistent tool, missing/unreachable referenced template); high −0.15 (contradictory instructions or malformed template contract); medium −0.10 (removed vocabulary used as operative dispatch, missing Constraints, vague instructions); low −0.05 (verified minor staleness with a specific behavioral impact). Do not double-count the same root cause. Include evidence and arithmetic per penalty; use `lisp_eval` for the calculation when available. No verified defects → 1.0.
4. Classify 0.00–0.19 as retirement candidate, 0.20–0.49 as critical revision, 0.50–0.79 as stale warning, 0.80–1.00 as active. A score is advisory: propose repair first and never delete or mark a skill deprecated without explicit operator approval. Explain when a low score is caused by multiple repairable faults.
5. Respond with staleness report, health score and traceable penalties, coverage limitations (checks not performed), and recommendations. For any unverified signal report `unverified` separately without a penalty. A score of 1.0 establishes only the absence of verified staleness defects, not skill effectiveness; route requests to improve behavior to `skill-maintenance-optimize`.

### skill-maintenance-optimize — Plan → Do → Check → Act (proposal only)

This loop runs within one session and ends in a **proposal**, never an applied change or a verdict. Skill evaluation belongs to the operator in `algedonic-review`'s gemba walk (operator ruling 2026-09-24; Goodhart's law): the session that designed a candidate cannot also be the judge that accepts it.

1. **Plan:** Read the canonical SKILL.md and referenced .j2 templates via `read_file`; obtain the operator's task/outcome and relevant historical constraints. Specify baseline behavior and a fixed, independently judged set of representative, negative, boundary and held-out tasks with expected results before revising anything. Select success, regression, safety and cost measures and a feasible run budget. `skill-maintenance-audit` and `skill-maintenance-validate` identify defects, but passing them does not establish task success. If the objective or oracle cannot be established, ask for it or return `unverified`; do not optimize to a proxy health score.
2. **Do:** Render `skill-maintenance-optimize` to lay out at most four genuinely different candidates: unchanged baseline, surgical repair, remove/merge/simplify, and replacement of the process architecture. Permit elimination of a template, reallocation of responsibilities or a new skill boundary if the task justifies it; preserve only externally required contracts. Record why a candidate class is inapplicable rather than forcing a change. For .j2 reasoning defects, invoke `skill-logic-audit` on the template as a leaf; this skill owns SKILL.md changes and integration. Do not use a legacy manifest as the process specification.
3. **Check (measure, do not judge):** Run baseline and candidate implementations against the **same** tasks and evaluator when a deterministic harness can run them; validate S1–S13/T1–T5 and render reachability for every finalist. Call `lisp_eval` to reconcile task IDs, run counts, arithmetic and hard-gate results from recorded data; retain logs under `~/Documents/zk-data/skills/skill-maintenance/{date}-{run}/`. A self-scored answer, static template overlap, or a green structural check is not evidence of improved task outcomes. Missing runs and unavailable harnesses (the local swarm runtime does not execute skills) are `unverified`, never wins.
4. **Formal gate when applicable:** Invoke `lean-prover` only if a candidate depends on a precisely stated finite decision rule or safety invariant whose proof changes the choice (e.g. no unapproved write transition). State assumptions, compile the exact declaration in the pinned Lean version, inspect `#print axioms` and negative controls, and test that the production decision rule matches the model. Lean cannot prove semantic quality or an absolute optimum. If no such obligation exists, record `not applicable`; if needed but uncheckable, record `unverified`.
5. **Act (file or drop):** A candidate that satisfies hard constraints and has measured evidence is written as a proposal — full diff, predeclared tasks, measured before/after (or `unverified`), open falsifiers — via `terminal` to `~/Documents/zk-data/curator/proposals/{skill}/{date}-{run}.json`. Otherwise drop it and keep the baseline. For a new falsifier, revise the design and rerun the same held-out cases at most once in this session. Do not edit the SKILL.md, record a verdict, or claim an improvement; the operator decides the proposal in the gemba walk, and only an accepted proposal is applied.

### skill-maintenance-coverage

1. Map task patterns against the existing skill corpus.
2. Classify each task pattern: covered, uncovered, or partial coverage.
3. For uncovered patterns, assess impact (critical/high/medium/low) and
   recommend action (create skill, extend skill, discover external, ignore).
4. For partial coverage, identify the missing aspects and the extension needed.
5. Respond with covered patterns, uncovered patterns, partial coverage, and
   recommendations.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `skill-maintenance-validate.j2` | Validate a skill or all skills against S1–S13 / T1–T5 with per-check evidence and fix suggestions. |
| `skill-maintenance-audit.j2` | SKILL.md-first staleness audit: verified dead tools, missing referenced templates, removed dispatch vocabulary, vague instructions, malformed templates; traceable health penalties and advisory recommendations. |
| `skill-maintenance-optimize.j2` | Compare distinct SKILL.md and companion-template architectures on fixed task outcomes, hard constraints and regressions, and package the measured comparison as a proposal for the operator's algedonic review; never select or apply a winner. |
| `skill-maintenance-build.j2` | Generate a complete skill (SKILL.md + .j2 templates) from a natural-language description. |
| `skill-maintenance-translate.j2` | Convert a classified source skill into the kask format, mapping source steps and tools to kask equivalents. |
| `skill-maintenance-coverage.j2` | Map task patterns against the skill corpus: covered, uncovered, partial — with impact and action recommendations. |

To render a template, call the `render_template` tool with the template ref (e.g., `skill-maintenance/skill-maintenance-validate`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `skill-maintenance-audit.j2`: `skill_name`,`workspace_context`
- `skill-maintenance-build.j2`: `skill_description`,`scope`
- `skill-maintenance-optimize.j2`: `skill_name`,`objective`,`baseline`,`tasks`,`candidates`,`observations`
- `skill-maintenance-coverage.j2`: `skill_catalog`,`task_patterns`
- `skill-maintenance-translate.j2`: `source_skill`,`target_domain`


## Constraints

- SKILL.md is the source of truth. When SKILL.md and templates disagree,
  SKILL.md wins.
- `lisp_eval` is available for deterministic computation. Use it for
  convergence signals, invariant checks, scoring, and arithmetic on
  structured data.
- The interpreter supports prefix `(+ a b)` and infix `a + b` operator
  notation. Use infix for simple scoring, prefix for complex nested logic.
- No `visibility` field in frontmatter.
- SKILL.md must not use removed vocabulary: `compute_ref`, `action:`,
  `template_ref` as a manifest dispatch key (the `render_template` parameter
  of the same name is the live contract), `convergence_signal`,
  `input_mapping`, `on_failure`, `ordinal:`, `category:`.
- Core skills (`core: true`) must have names in `CORE_SKILL_NAMES`.
- Material SKILL.md process changes are never self-accepted. This skill
  files them as proposals under `zk-data/curator/proposals/`; the operator
  accepts or rejects them in `algedonic-review`'s gemba walk, where the
  verdict is recorded. Measurements travel with the proposal as evidence;
  where no harness could run, the proposal says **unverified**.
  An edit that cannot name a measurable behavior change is a documentation
  edit — label it as such.
