---
name: skill-maintenance
core: true
description: "Skill lifecycle management. SKILL.md is the canonical source of truth; .j2 templates are companion resources. Audit staleness, coverage gaps, and quality. Validate, build, translate, and prune skills."
---

# Skill Maintenance

Skill lifecycle management and maintenance. SKILL.md is the canonical source
of truth — the process instructions the agent reads and follows. .j2 templates
are companion resources that define prompt structure. Audit staleness,
coverage gaps, and quality. Validate, build, translate, and prune skills.

## The skill model

A kask skill has two artifacts:

```
.agents/skills/<name>/
├── SKILL.md              # Process instructions (source of truth)
├── <phase>.j2            # Prompt templates (companion resources)
```

No manifest.yaml, no template manifest, no process executor. The agent reads
the SKILL.md, follows its instructions, and calls tools (`lisp_eval`, MCP
tools, `read_file`, `skill`) as directed.

## When to Use

- When you need to validate a skill's SKILL.md structure and template quality.
- When you need to scaffold a new skill (SKILL.md + templates) from a
  natural-language description.
- When you need to translate a classified source skill into the kask format.
- When you need to audit skills for staleness signals and health scoring.
- When you need to map task patterns against the skill corpus for coverage gaps.

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
     MCP tools, `read_file`, `skill`) — not abstract "the system will" language
   - **S8**: SKILL.md has a "Constraints" section
   - **S9**: No `visibility` field in frontmatter (removed — no marketplace)
   - **S10**: No `compute_ref`, `action:`, `template_ref`, `convergence_signal`,
     `input_mapping`, `on_failure` references (manifest vocabulary is gone)
   - **S11**: If `core: true` is declared, the name must be in
     `CORE_SKILL_NAMES` (enforced by `agent_skills` at load time)
   - **T1**: Each `.j2` template referenced in SKILL.md instructions exists
     in the skill directory
   - **T2**: Each `.j2` template has a comment header describing its purpose
   - **T3**: Each `.j2` template defines expected output fields (as comments
     or schema description)
   - **T4**: No `[inference]` frontmatter in .j2 templates (templates are
     resources, not executed code)
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
   - Comment header with purpose
   - Jinja2 variables for context
   - Expected JSON output shape
5. Derive the PDCA shape from the skill's ontological anchors (see create-skill).
6. Respond with the SKILL.md content, template contents, and validation status.

### skill-maintenance-translate

1. Convert a classified source skill (e.g., from another agent system) into
   the kask format: SKILL.md + .j2 templates.
2. Map source process steps to SKILL.md instruction steps.
3. Map source tool calls to kask tools:
   - Deterministic computation → `lisp_eval`
   - Data retrieval → appropriate MCP tool
   - Skill composition → `skill` tool
   - File operations → `read_file`, `write_file`, `edit_file`
4. Create .j2 templates for reasoning steps that need structured prompts.
5. Mark any source concepts with no kask equivalent as
   `[unresolved: no kask equivalent for <source_ref>]`.
6. Respond with the SKILL.md, templates, and a translation summary.

### skill-maintenance-audit

1. Audit skills for staleness signals:
   - SKILL.md references tools that no longer exist (renamed MCP tools,
     removed agent tools)
   - SKILL.md instructions reference .j2 templates that are missing
   - SKILL.md still uses manifest vocabulary (`action:`, `compute_ref:`,
     `template_ref:`, `convergence_signal`) — these are deprecated
   - SKILL.md has no "Constraints" section
   - SKILL.md instructions are vague ("the system will analyze...") instead
     of concrete ("call `lisp_eval` with form...")
   - .j2 templates have `[inference]` frontmatter (deprecated)
2. Compute health scores from 0.0 to 1.0 using weighted penalties.
3. Recommend deprecation or retirement based on health score thresholds:
   - 0.00-0.19: retirement
   - 0.20-0.49: critical — needs immediate attention
   - 0.50-0.79: stale warning
   - 0.80-1.00: active
4. Cite every finding from a file path and line number — never speculate.
5. Respond with staleness report, health scores, and recommendations.

### skill-maintenance-coverage

1. Map task patterns against the existing skill corpus.
2. Classify each task pattern: covered, uncovered, or partial coverage.
3. For uncovered patterns, assess impact (critical/high/medium/low) and
   recommend action (create skill, extend skill, discover external, ignore).
4. For partial coverage, identify the missing aspects and the extension needed.
5. Respond with covered patterns, uncovered patterns, partial coverage, and
   recommendations.

## Migration: manifest-based skills → SKILL.md process

Existing skills that still have `kask/registry/manifests/<name>.yaml` need
migration to the new model. The migration process:

1. **Read the manifest** to understand the skill's PDCA shape, step sequence,
   and tool calls.
2. **Read the .j2 templates** to understand each step's prompt structure and
   expected output.
3. **Write the new SKILL.md**:
   - Convert each `action: select` step into an instruction: "Read
     `<template>.j2` and produce analysis following its output schema."
   - Convert each `action: execute` step into an instruction: "Call `<mcp_tool>`
     with `<parameters>`."
   - Convert each `action: compute` step with `compute_ref: lisp.eval` into:
     "Call `lisp_eval` with form: `<form>` and env: `<env>`."
   - Convert each `action: loop` with `convergence_signal` into a convergence
     instruction with an `lisp_eval` check.
   - Convert each `on_failure: { action: report }` into: "If the tool call
     fails, call `curator_report_skill_use_issue` with..."
   - Convert each `template_ref` to another skill into: "Call the `skill` tool
     with name: `<other_skill>`."
4. **Move .j2 templates** from `kask/registry/templates/<name>/` to
   `.agents/skills/<name>/`.
5. **Delete the manifest**: `kask/registry/manifests/<name>.yaml`.
6. **Validate** the migrated skill with `skill-maintenance-validate`.

## Constraints

- SKILL.md is the source of truth. When SKILL.md and templates disagree,
  SKILL.md wins.
- `lisp_eval` is available for deterministic computation. Use it for
  convergence signals, invariant checks, scoring, and arithmetic on
  structured data.
- The interpreter supports prefix `(+ a b)` and infix `a + b` operator
  notation. Use infix for simple scoring, prefix for complex nested logic.
- No `visibility` field in frontmatter (marketplace removed).
- No manifest vocabulary in SKILL.md: `action:`, `compute_ref:`,
  `template_ref:`, `convergence_signal`, `input_mapping`, `on_failure`,
  `ordinal:`, `category:`.
- Core skills (`core: true`) must have names in `CORE_SKILL_NAMES`.