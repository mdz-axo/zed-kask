---
name: skill-maintenance
description: Skill lifecycle management and maintenance. Registry crate (manifest.yaml + *.j2) is the canonical source of truth; SKILL.md is a generated companion. Audit staleness, coverage gaps, and quality. List, build, validate, install, translate, and prune skills. Pairs with skill-discovery and skill-bundler.
---

# Skill Maintenance

Skill lifecycle management and maintenance. Registry crate (manifest.yaml + *.j2) is the canonical source of truth; SKILL.md is a generated companion. Audit staleness, coverage gaps, and quality. List, build, validate, install, translate, and prune skills. Pairs with skill-discovery and skill-bundler.

## When to Use

- When you need to validate skills against the registry-first model, checking manifest structure, .j2 frontmatter, and cross-artifact consistency.
- When you need to validate process manifests (registry/manifests/*.yaml) for executor compliance: canonical actions, gas/rjoule budgets, convergence blocks, and template ref resolvability.
- When you need to scaffold a new registry crate from a natural language user description, including a compliant process manifest with gas/rjoule/convergence blocks.
- When you need to translate a classified source skill into a hKask registry crate (manifest.yaml + .j2 templates) with canonical actions and proper budgets.
- When you need to reverse-translate a registry crate into a SKILL.md companion for the Zed coding agent.
- When you need to synthesize the "When to Use" and "Instructions" prose sections of a SKILL.md from a registry crate.
- When you need to audit registry crates for staleness signals, health scoring, and deprecation recommendations.
- When you need to map task patterns against the skill corpus to identify coverage gaps.

## Instructions

### skill-maintenance-validate

1. Validate the specified skill or all skills in the registry directory against R1-R12 registry checks, Z1-Z8 companion checks, X1-X4 cross-artifact checks, and E1-E10 executor compliance checks.
2. Evaluate every check for every targeted skill without omissions, including invariant X5: every `.agents/skills/<name>/` must have a matching `registry/manifests/<name>.yaml`, and vice versa. Report exact mismatches by name.
3. For executor compliance (E1-E10), verify that every process manifest uses only canonical actions, has gas/rjoule blocks with adequate caps, has a convergence block (for skill category), has valid category, and has resolvable template_refs.
4. Include specific evidence for any fail results.
5. Provide actionable fix suggestions for any failures, including mapping non-canonical actions to their canonical equivalents.
6. Respond with a JSON object containing validation results, pass/fail counts (including executor_compliance), and fix suggestions.

### skill-maintenance-build

1. Generate a complete registry crate (manifest.yaml and .j2 templates) from the user's natural language description.
2. Ensure the skill name is lowercase, hyphenated, 2-40 characters, verb-noun or noun-noun, and lacks reserved prefixes.
3. Create at least one .j2 template with valid [inference] frontmatter and a Jinja2 body containing a system prompt and JSON output schema.
4. Generate a process manifest (registry/manifests/<name>.yaml) with: `category: skill`, `convergence:` block (threshold 0.05-0.30, max_iterations, min_iterations, convergence_field, on_not_reached), `gas:` block (cap proportional to step count), `rjoule:` block (cap > 0 if inference is used), and `steps:` array using only canonical actions.
5. Derive a SKILL.md companion from the completed registry crate.
6. Respond with a JSON object containing the manifest, process manifest, template bodies, SKILL.md outline, and validation status (including actions_canonical, gas_block_present, rjoule_block_present, convergence_block_present).

### skill-maintenance-translate

1. Convert the classified source skill into a hKask registry crate (manifest.yaml + .j2 templates) plus a process manifest (registry/manifests/<name>.yaml).
2. Produce one .j2 file per classified step, mapping cognitive steps to KnowAct, workflow steps to WordAct or FlowDef, reference content to RenderAct, and guardrails to visibility, energy_cap, and constraints.
3. Map source actions to canonical hKask actions using the action mapping table (e.g., `call` → `execute`, `classify` → `select`, `run_command` → `execute`, `check` → `validate`).
4. Generate gas/rjoule budgets based on the translated step count and inference usage (simple: gas 5K-10K/rjoule 1-2; multi-step: gas 50K-150K/rjoule 3-5; media: gas 100K+/rjoule 5+).
5. Generate a convergence block with threshold appropriate to the skill type (0.05-0.15 for precise, 0.20-0.30 for broad).
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
5. Respond with a JSON object containing staleness report, health scores, coverage gaps, and deprecation recommendations.

### skill-maintenance-coverage

1. Map task patterns against the existing skill corpus to determine full, partial, or no coverage.
2. Classify each task pattern into exactly one category: covered, uncovered, or partial coverage.
3. For uncovered patterns, assess impact (critical/high/medium/low) and recommend action (create_skill, extend_skill, discover_external, ignore).
4. For partial coverage, identify the missing aspects and the extension needed.
5. Respond with a JSON object containing covered patterns, uncovered patterns, partial coverage, and recommendations.

### skill-maintenance-convergence-check

1. Compute a normalized convergence metric in [0,1] for the maintenance PDCA cycle, where 0 means critical staleness signals are resolved.
2. Start at 1.0 and adjust downward based on audit and coverage results: critical signals keep metric >= 0.7, medium/low findings set metric in [0.2, 0.6], no critical/high with bounded gaps sets metric <= 0.1.
3. Identify unresolved critical signals and blockers preventing convergence.
4. Return a JSON object containing convergence_metric, rationale, blockers, and unresolved_critical_signals.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `skill-maintenance-validate.j2` | KnowAct | Validate skills against registry format and quality checks. Check manifest structure, .j2 frontmatter (template_type, contract, visibility, energy_cap). SKILL.md is validated as secondary companion. |
| `skill-maintenance-build.j2` | KnowAct | Scaffold a new registry crate from a user description. Generate manifest.yaml with crate metadata, template entries, and lexicon_terms. Generate companion SKILL.md from the registry crate. Validate and confirm before writing. |
| `skill-maintenance-translate.j2` | KnowAct | Forward translation: convert a classified source skill into a hKask registry crate (manifest.yaml + *.j2 templates). Map source elements to hKask equivalents, drop concepts with no equivalent, produce validated output with translation summary. |
| `skill-maintenance-reverse.j2` | KnowAct | Reverse translation: generate a SKILL.md companion from a registry crate. Read manifest.yaml for crate metadata, read .j2 templates for methodology, produce a markdown companion suitable for the Zed coding agent. |
| `skill-maintenance-prose.j2` | KnowAct | Prose-only derivation: synthesize the "When to Use" and "Instructions" sections of a SKILL.md from a registry crate, emitted as raw markdown. Used by the skill-maintenance skill or agent panel alongside the mechanically-built skeleton (frontmatter, templates table, constraints) — the LLM only writes the prose that needs synthesis, not the structural parts copied from the registry. |
| `skill-maintenance-audit.j2` | KnowAct | Run staleness and health audit for target scope. Checks R1-R12 registry rules, Z1-Z8 companion checks, X1-X4 cross-artifact checks. Used by the FlowDef manifest as step 1 of the maintenance PDCA loop. |
| `skill-maintenance-coverage.j2` | KnowAct | Run corpus coverage analysis for uncovered/partial capabilities. Maps common task patterns against the existing skill corpus, identifies what is covered, uncovered, and partial. Used by the FlowDef manifest as step 2 of the maintenance PDCA loop. |
| `skill-maintenance-convergence-check.j2` | KnowAct | Compute normalized convergence metric for maintenance PDCA cycles. Measures critical signal count, coverage gaps, and regression library growth. Used by the FlowDef manifest as step 3 of the maintenance PDCA loop. |
| `logic-load-goal.j2` | WordAct | Parse the annotated goal: block from a .j2 or manifest.yaml file. (logic_audit mode, folded from skill-logic-audit) |
| `logic-critique-template.j2` | KnowAct | Adversarial critique of a template body against its stated goal. For each flaw, provide location, claim, anchor to goal, severity, and suggested fix. (logic_audit mode) |
| `logic-critique-critique.j2` | KnowAct | Soundness filter — separate valid, goal-anchored concerns from spurious ones. (logic_audit mode) |
| `logic-compose-proposal.j2` | KnowAct | Compose a concrete revised artifact and unified diff from calibrated concerns. (logic_audit mode) |
| `logic-user-choice.j2` | KnowAct | Present the proposal to the user and capture accept/reject/counter-proposal. (logic_audit mode) |
| `logic-convergence-check.j2` | KnowAct | Compute convergence metric for logic audit cycle. Converged when no material flaws remain. (logic_audit mode) |

## Constraints

- `skill-maintenance-validate.j2`: Public. R1-R12 mandatory; Z1-Z8 secondary; X1-X4 cross-artifact; E1-E10 executor compliance mandatory. R1-R5 failures are critical; E1/E2/E4/E5/E6/E7/E9 failures are critical; Z5/Z6/Z7 failures are high; missing SKILL.md (Z1) is info, not failure.
- `skill-maintenance-build.j2`: Public. Name must be lowercase, hyphenated, 2-40 chars, verb-noun or noun-noun, no reserved prefixes. Process manifest must have gas/rjoule/convergence blocks and canonical actions.
- `skill-maintenance-translate.j2`: Public. template_type must be KnowAct/WordAct/FlowDef/RenderAct; visibility must be Private/Public/Shared; energy_cap must be 2048-8192. Source actions must be mapped to canonical actions. Process manifest must have gas/rjoule/convergence blocks.
- `skill-maintenance-reverse.j2`: Public. Every instruction must trace to a manifest field or .j2 body — do not invent content.
- `skill-maintenance-prose.j2`: Public. Output raw markdown only — no JSON, code fences, frontmatter, or structural sections.
- `skill-maintenance-audit.j2`: Public. Every finding must cite a FlowDef manifest field, .j2 contract/metadata, or grep-verifiable Rust code path. Recommendations based solely on SKILL.md must be marked confidence: Hypothesis (Speculative) at maximum.
- `skill-maintenance-coverage.j2`: Public. Every task pattern must appear in exactly one of: covered, uncovered, or partial. Do not recommend `ignore` for uncovered patterns with critical or high impact.
- `skill-maintenance-convergence-check.j2`: Public. Metric in [0,1]; threshold 0.15; max 3 iterations.
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
| `flowdef` | FlowDef | Recursively execute a `.yaml` sub-manifest as a nested cascade with gas/rjoule budget inheritance |
| `loop` | Control Flow | Re-enter cascade from a target ordinal |
| `choice` | Control Flow | Evaluate condition and branch to a target ordinal |
| `abort` | Control Flow | Exit with success (converged) |
| `escalate` | Control Flow | Exit with error (blocked) |

The `evaluate` action has been removed. Manifests using `evaluate` must use
`select` (KnowAct) or `flowdef` (FlowDef recursion) instead.

## Gas/rJoule Budget Requirements

Every process manifest must declare energy budgets:

- **`gas:` block** (compute cycles) — required with `cap > 0`, `cost_per_iteration`, `alert_threshold`, `hard_limit`. Gas is cheap compute; 250,000 gas cycles ≈ 1 rJoule.
- **`rjoule:` block** (inference energy) — required. `cap > 0` if any step uses `action: select` (inference). `cap: 0` is acceptable only for manifests with no inference steps.

Budget guidelines:

| Skill complexity | Step count | gas.cap | rjoule.cap |
|-----------------|-----------|---------|------------|
| Simple KnowAct | 1-3 | 5,000-10,000 | 1-2 |
| Multi-step FlowDef | 4-7 | 50,000-150,000 | 3-5 |
| Media generation | 5+ | 100,000+ | 5+ |
| Infrastructure (no inference) | any | 10,000-50,000 | 0 |

Sub-manifests referenced by `action: flowdef` steps inherit the parent's
remaining gas/rjoule budget (capped to the sub-manifest's declared budget if
smaller). Sub-manifests should declare their own `gas` and `rjoule` blocks.

## Convergence Block Requirements

Every `category: skill` manifest must have a `convergence:` block:

```yaml
convergence:
  threshold: 0.15           # 0.05-0.30; 0.05-0.15 precise, 0.20-0.30 broad
  improvement_gate: threshold_only  # threshold_only | both | either
  max_iterations: 3         # max PDCA iterations before forced exit
  min_iterations: 1          # min iterations before exit allowed
  convergence_field: step_N_result.convergence_metric  # context field to read
  on_not_reached: escalate   # abort | escalate
```

Non-skill categories (`qa-script`, `runtime-config`, `daemon-process`,
`pipeline`) may have convergence blocks but are not required to.

## Pattern A Template Types

The four Pattern A template types map to actions as follows:

| Pattern A Type | Action | File format |
|---------------|--------|------------|
| KnowAct | `select` | `.j2` (rendered + inference + JSON parse) |
| WordAct | `populate` | `.j2` (rendered without inference) |
| FlowDef | `flowdef` | `.yaml` (sub-manifest with own steps/gas/convergence) |
| RenderAct | `render` | `.j2` or `.yaml` (rendered without inference — reference content, macros) |
