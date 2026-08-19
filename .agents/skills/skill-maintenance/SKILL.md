---
name: skill-maintenance
core: true
description: "Skill lifecycle management. Registry crate (manifest.yaml + *.j2) is the canonical source of truth; SKILL.md is a generated companion. Audit staleness, coverage gaps, and quality. List, build, validate, install, translate, and prune skills."
---

# Skill Maintenance

Skill lifecycle management and maintenance. Registry crate (manifest.yaml + *.j2) is the canonical source of truth; SKILL.md is a generated companion. Audit staleness, coverage gaps, and quality. List, build, validate, install, translate, and prune skills. Pairs with skill-discovery and skill-bundler.

## When to Use

- When you need to validate skills against the registry-first model, checking manifest structure, .j2 frontmatter, and cross-artifact consistency.
- When you need to validate process manifests (registry/manifests/*.yaml) for executor compliance: canonical actions, rjoule budgets, convergence blocks, and template ref resolvability.
- When you need to scaffold a new registry crate from a natural language user description, including a compliant process manifest with rjoule/convergence blocks.
- When you need to translate a classified source skill into a hKask registry crate (manifest.yaml + .j2 templates) with canonical actions and proper budgets.
- When you need to reverse-translate a registry crate into a SKILL.md companion for the Zed coding agent.
- When you need to synthesize the "When to Use" and "Instructions" prose sections of a SKILL.md from a registry crate.
- When you need to audit registry crates for staleness signals, health scoring, and deprecation recommendations.
- When you need to map task patterns against the skill corpus to identify coverage gaps.

## Instructions

### skill-maintenance-validate

1. Validate the specified skill or all skills in the registry directory against R1-R12 registry checks, Z1-Z8 companion checks, X1-X4 cross-artifact checks, and E1-E11 executor compliance checks.
2. Evaluate every check for every targeted skill without omissions, including invariant X4: every `.agents/skills/<name>/` must have a matching `registry/manifests/<name>.yaml`, and vice versa. Report exact mismatches by name.
3. For executor compliance (E1-E11), verify that every process manifest uses only canonical actions, has an rjoule block with an adequate cap, has a convergence block (for skill category), has valid category, has resolvable template_refs, and has a `ledger.span_namespace` equal to `reg.skill.<manifest.id>` with no abolished `spans:` list (E11).
4. **Visual artifact surfacing check (E12):** For any skill whose template contracts or SKILL.md description mention a visual artifact (Mermaid diagram, chart, map, sankey, quadrant chart, or any renderable output), verify the process manifest has a `render` step (action: render) whose ordinal is the highest among steps that produce a `step_N_result` (the `loop` action does not produce one). The render step must surface the artifact as a fenced ```mermaid block in its output. Flag skills where the artifact is generated in an intermediate `select` step but not surfaced by a final `render` step — the diagram will be buried in an intermediate `step_N_result` and never reach the chat stream. See the "Visual artifact surfacing" section in create-skill for the full pattern.
5. Include specific evidence for any fail results.
6. Provide actionable fix suggestions for any failures, including mapping non-canonical actions to their canonical equivalents.
7. Respond with a JSON object containing validation results, pass/fail counts (including executor_compliance), and fix suggestions.

### skill-maintenance-build

1. Generate a complete registry crate (manifest.yaml and .j2 templates) from the user's natural language description.
2. Ensure the skill name is lowercase, hyphenated, 2-40 characters, verb-noun or noun-noun, and lacks reserved prefixes.
3. Create at least one .j2 template with valid [inference] frontmatter and a Jinja2 body containing a system prompt and JSON output schema.
4. Generate a process manifest (registry/manifests/<name>.yaml) with: `category: skill`, `convergence:` block (convergence_mode, cauchy_epsilon, cauchy_window, max_iterations, min_iterations, on_not_reached), `rjoule:` block (cap > 0 if inference is used), `steps:` array using only canonical actions (each step with `timeout_seconds`), and a `ledger:` block with `span_namespace: reg.skill.<name>` (CI-enforced; no `spans:` list).
5. **Visual artifact surfacing:** if any template produces a Mermaid diagram, chart, or visual artifact (detectable from the template's contract output fields or the skill description mentioning "diagram", "chart", "visual", or "renders natively in Zed"), add a final `render` step (action: render, renderer: minijinja) with a pure Jinja2 template (no frontmatter) that wraps the artifact in a fenced ```mermaid block. The render step's ordinal must be the highest among steps that produce a `step_N_result` (place it before the `loop` step). See the "Visual artifact surfacing" section in create-skill for the full pattern.
6. Derive a SKILL.md companion from the completed registry crate.
7. Respond with a JSON object containing the manifest, process manifest, template bodies, SKILL.md outline, and validation status (including actions_canonical, rjoule_block_present, convergence_block_present).

### skill-maintenance-translate

1. Convert the classified source skill into a hKask registry crate (manifest.yaml + .j2 templates) plus a process manifest (registry/manifests/<name>.yaml).
2. Produce one .j2 file per classified step, mapping cognitive steps to KnowAct, workflow steps to WordAct or FlowDef, reference content to RenderActand guardrails to visibility and constraints.
3. Map source actions to canonical hKask actions using the action mapping table (e.g., `call` → `execute`, `classify` → `select`, `run_command` → `execute`, `check` → `validate`).
4. Generate rjoule budgets based on the translated step count and inference usage (simple: rjoule 1-3; multi-step: rjoule 3-5; media: rjoule 5+).
5. Generate a convergence block with `convergence_mode: "cauchy"`, `cauchy_epsilon: 0.03`, `cauchy_window: 3`, `max_iterations: 10`, `min_iterations: 2`.
6. Map source state to .j2 contract input/output, user-confirmation gates to visibility, and domain references using the domain substitution table.
7. Mark any references with no hKask equivalent as `[unresolved: no hKask equivalent for <source_ref>]`.
8. Respond with a JSON object containing the manifest, process manifest, templates, derived SKILL.md, and a translation summary detailing preserved, adapted, dropped, unresolved elements, and action mappings.

### skill-maintenance-reverse

1. Read the provided manifest.yaml and .j2 template files for the target skill.
2. Generate a SKILL.md companion file with frontmatter, title, description, "When to Use", "Instructions", "Registry Templates" table, and "Constraints".
3. Synthesize the "When to Use" section from template descriptions and system prompts.
4. Extract imperative steps for the "Instructions" section from each .j2's system prompt body.
5. Emit warnings for empty system prompts, missing .j2 files, invalid template types, or missing vocabulary terms.
6. Respond with a JSON object containing the complete SKILL.md markdown content and any warnings.

### skill-maintenance-prose

1. Read the provided manifest.yaml and .j2 template contents for the target skill.
2. Synthesize the "When to Use" section from template descriptions and .j2 system prompts, providing one bullet per distinct trigger.
3. Extract imperative steps for the "Instructions" section from each .j2's system prompt body, preserving template order from the manifest.
4. Ensure every instruction traces to a manifest field or .j2 body without inventing content.
5. Output raw markdown only, containing exactly the "When to Use" and "Instructions" sections, without JSON, code fences, or structural sections.

### skill-maintenance-audit

1. Audit registry crates for staleness signals: manifest validity, .j2 contract drift, template_type correctness, FlowDef tool/template validation, and health scoring.
2. Apply the staleness signal table (Critical/High/Medium/Low severity) and compute health scores from 0.0 to 1.0 using weighted penalties.
3. Recommend deprecation or retirement based on health score thresholds (0.00-0.19 retirement, 0.20-0.49 critical, 0.50-0.79 stale warning, 0.80-1.00 active).
4. Cite every finding from a FlowDef manifest field, .j2 contract/metadata, or grep-verifiable Rust code path — never from SKILL.md alone.
5. **Visual artifact surfacing audit:** for any skill whose templates produce a Mermaid diagram, chart, or visual artifact, check that the process manifest includes a `render` step that surfaces the artifact as the cascade's final output. A skill that generates a diagram in an intermediate `select` step but lacks a surfacing `render` step has a Medium-severity staleness signal: the diagram is silently dropped and the user never sees it. This is the E12 validate check applied as an audit finding.
6. Respond with a JSON object containing staleness report, health scores, coverage gaps, and deprecation recommendations.

### skill-maintenance-coverage

1. Map task patterns against the existing skill corpus to determine full, partial, or no coverage.
2. Classify each task pattern into exactly one category: covered, uncovered, or partial coverage.
3. For uncovered patterns, assess impact (critical/high/medium/low) and recommend action (create_skill, extend_skill, discover_external, ignore).
4. For partial coverage, identify the missing aspects and the extension needed.
5. Respond with a JSON object containing covered patterns, uncovered patterns, partial coverage, and recommendations.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `skill-maintenance-validate.j2` | KnowAct | Validate skills against registry format and quality checks. Check manifest structure, .j2 frontmatter (template_type, contract, visibility). SKILL.md is validated as secondary companion. |
| `skill-maintenance-build.j2` | KnowAct | Scaffold a new registry crate from a user description. Generate manifest.yaml with crate metadata, template entries, and . Generate companion SKILL.md from the registry crate. Validate and confirm before writing. |
| `skill-maintenance-translate.j2` | KnowAct | Forward translation: convert a classified source skill into a hKask registry crate (manifest.yaml + *.j2 templates). Map source elements to hKask equivalents drop concepts with no equivalent, produce validated output with translation summary. |
| `skill-maintenance-reverse.j2` | KnowAct | Reverse translation: generate a SKILL.md companion from a registry crate. Read manifest.yaml for crate metadata, read .j2 templates for methodology produce a markdown companion suitable for the Zed coding agent. |
| `skill-maintenance-prose.j2` | KnowAct | Prose-only derivation: synthesize the "When to Use" and "Instructions" sections of a SKILL.md from a registry crate, emitted as raw markdown. Used by the skill-maintenance skill or agent panel alongside the mechanically-built skeleton (frontmatter, templates table, constraints) — the LLM only writes the prose that needs synthesis, not the structural parts copied from the registry. |
| `skill-maintenance-audit.j2` | KnowAct | Run staleness and health audit for target scope. Checks R1-R12 registry rules, Z1-Z8 companion checks, X1-X4 cross-artifact checks. Used by the FlowDef manifest as step 1 of the maintenance PDCA loop. |
| `skill-maintenance-coverage.j2` | KnowAct | Run corpus coverage analysis for uncovered/partial capabilities. Maps common task patterns against the existing skill corpus, identifies what is covered, uncovered, and partial. Used by the FlowDef manifest as step 2 of the maintenance PDCA loop. |

## Constraints

- rJoule cap: 2 per invocation. Maximum 10 iterations.
- `skill-maintenance-validate.j2`: Public. R1-R12 mandatory; Z1-Z8 secondary; X1-X4 cross-artifact; E1-E16 executor compliance mandatory. R1-R5 failures are critical; E1/E2/E4/E5/E6/E7/E9/E11/E12/E16 failures are critical; E15 (on_failure config) failures are medium; E12 failures are critical; E13/E14 failures are high; E12 visual artifact surfacing failures are high (diagram silently dropped — user never sees visualization); Z5/Z6/Z7 failures are high; missing SKILL.md (Z1) is info, not failure.
- `skill-maintenance-build.j2`: Public. Name must be lowercase, hyphenated, 2-40 chars, verb-noun or noun-noun, no reserved prefixes. Process manifest must have rjoule/convergence blocks, canonical actions, and a `ledger:` block with `span_namespace: reg.skill.<manifest.id>` (no abolished `spans:` list).
- `skill-maintenance-translate.j2`: Public. template_type must be KnowAct/WordAct/FlowDef/RenderAct; visibility must be Private/Public/Shared Source actions must be mapped to canonical actions. Process manifest must have rjoule/convergence blocks.
- `skill-maintenance-reverse.j2`: Public. Every instruction must trace to a manifest field or .j2 body — do not invent content.
- `skill-maintenance-prose.j2`: Public. Output raw markdown only — no JSON, code fences, frontmatter, or structural sections.
- `skill-maintenance-audit.j2`: Public. Every finding must cite a FlowDef manifest field, .j2 contract/metadata, or grep-verifiable Rust code path. Recommendations based solely on SKILL.md must be marked confidence: Hypothesis (Speculative) at maximum.
- `skill-maintenance-coverage.j2`: Public. Every task pattern must appear in exactly one of: covered, uncovered, or partial. Do not recommend `ignore` for uncovered patterns with critical or high impact.
- **`lisp.eval` is available for custom deterministic compute steps.** When auditing or building skill manifests, recommend `compute_ref: lisp.eval` for skills that need custom convergence formulas, scoring functions, or data transformations that don't fit the built-in `compute_ref`s. No Rust change needed — the manifest is the unit of authorship. Security: gated to `category: skill` manifests only. The interpreter supports both prefix (`(+ a b)`) and infix (`a + b`) operator notation — recommend infix for simple scoring expressions, prefix for complex nested logic.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.

## Canonical Action Set (ManifestExecutor)

The ManifestExecutor dispatches only these actions. Non-canonical actions cause
runtime errors (`Unknown manifest step action`). Every step in a process manifest
must use one of these:

| Action | Pattern A Type | Description |
|--------|---------------|-------------|
| `select` | KnowAct | Render `.j2` template + inference + parse JSON response |
| `populate` | WordAct | Render `.j2` template without inference (deterministic text) |
| `compute` | Deterministic | Invoke a `hkask_forecast::*` math primitive via `compute_ref` |
| `execute` | MCP Tool | Invoke an MCP tool (requires `mcp:` field) |
| `feedback` | MCP Tool | Emit Regulation feedback via MCP tool |
| `validate` | MCP Tool | Validate a contract or condition via MCP tool |
| `retrieve` | MCP Tool | Retrieve data via MCP tool (e.g., semantic search) |
| `render` | RenderAct | Render `.j2` or `.yaml` without inference (reference content, macros) |
| `flowdef` | FlowDef | Recursively execute a `.yaml` sub-manifest as a nested cascade with rjoule budget inheritance |
| `loop` | Control Flow | Re-enter cascade from a target ordinal |
| `choice` | Control Flow | Evaluate condition and branch to a target ordinal |
| `abort` | Control Flow | Exit with success (converged) |
| `escalate` | Control Flow | Exit with error (blocked) |

The `evaluate` action has been removed. Manifests using `evaluate` must use
`select` (KnowAct) or `flowdef` (FlowDef recursion) instead.

## rJoule Budget and Timeout Requirements

Every process manifest must declare an inference-energy budget. The gas system
and max-tokens caps are deprecated and no longer parsed by the executor — do not
add `gas:` blocks to new manifests.

- **`rjoule:` block** (inference energy, a USD budget: 1 rJoule = $1) — required. `cap > 0` if any step uses `action: select` (inference). `cap: 0` is acceptable only for manifests with no inference steps. Each inference call's observed USD cost is charged to this budget; exceeding the cap with `hard_limit: true` trips a `BudgetExhaustion` exit.
- **`timeout_seconds`** — the runaway cutoff. Every step should declare one; the executor aborts a step that exceeds it (with `error_handling.on_timeout: retry` governing retries). Convergence `max_iterations` bounds loop re-entry.

Budget guidelines:

| Skill complexity | Step count | rjoule.cap |
|-----------------|-----------|------------|
| Simple KnowAct | 1-3 | 1-3 |
| Multi-step FlowDef | 4-7 | 3-5 |
| Media generation | 5+ | 5+ |
| Infrastructure (no inference) | any | 0 |

Sub-manifests referenced by `action: flowdef` steps inherit the parent's
remaining rJoule budget (capped to the sub-manifest's declared budget if
smaller). Sub-manifests should declare their own `rjoule` block.

## Convergence Block Requirements

Every `category: skill` manifest must have a `convergence:` block. Convergence is detected deterministically via the Cauchy criterion — the iterates have stopped moving. No LLM convergence-check template is used.

Non-skill categories (`qa-script`, `runtime-config`, `daemon-process`,
`pipeline`) may have convergence blocks but are not required to.

## Pattern A Template Types

The four Pattern A template types map to actions as follows:

| Pattern A Type | Action | File format |
|---------------|--------|------------|
| KnowAct | `select` | `.j2` (rendered + inference + JSON parse) |
| WordAct | `populate` | `.j2` (rendered without inference) |
| FlowDef | `flowdef` | `.yaml` (sub-manifest with own steps/rjoule/convergence) |
| RenderAct | `render` | `.j2` or `.yaml` (rendered without inference — reference content, macros) |
