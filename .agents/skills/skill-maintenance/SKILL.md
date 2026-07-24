---
name: skill-maintenance
visibility: public
description: "Skill lifecycle management and maintenance. Registry crate (manifest.yaml + *.j2) is the canonical source of truth; SKILL.md is a generated companion. Audit staleness, coverage gaps, and quality. List, build, validate, install, translate, and prune skills. Pairs with skill-discovery and skill-bundler.
"
---

# Skill Maintenance

Skill lifecycle management and maintenance. Registry crate (manifest.yaml + *.j2) is the canonical source of truth; SKILL.md is a generated companion. Audit staleness, coverage gaps, and quality. List, build, validate, install, translate, and prune skills. Pairs with skill-discovery and skill-bundler.


## When to Use

- When you need to validate skills against the registry-first model, checking manifest structure, .j2 frontmatter, and cross-artifact consistency.
- When you need to scaffold a new registry crate from a natural language user description.
- When you need to translate a classified source skill into a hKask registry crate (manifest.yaml + .j2 templates).
- When you need to reverse-translate a registry crate into a SKILL.md companion for the Zed coding agent.
- When you need to synthesize the "When to Use" and "Instructions" prose sections of a SKILL.md from a registry crate.

## Instructions

### skill-maintenance-validate

1. Validate the specified skill or all skills in the registry directory against R1-R12 registry checks, Z1-Z8 companion checks, and X1-X4 cross-artifact checks.
2. Evaluate every check for every targeted skill without omissions, including invariant X5: every `.agents/skills/<name>/` must have a matching `registry/manifests/<name>.yaml`, and vice versa. Report exact mismatches by name.
3. Include specific evidence for any fail results.
4. Provide actionable fix suggestions for any failures.
5. Respond with a JSON object containing validation results, pass/fail counts, and fix suggestions.

### skill-maintenance-build

1. Generate a complete registry crate (manifest.yaml and .j2 templates) from the user's natural language description.
2. Ensure the skill name is lowercase, hyphenated, 2-40 characters, verb-noun or noun-noun, and lacks reserved prefixes.
3. Create at least one .j2 template with valid [inference] frontmatter and a Jinja2 body containing a system prompt and JSON output schema.
4. Derive a SKILL.md companion from the completed registry crate.
5. Respond with a JSON object containing the manifest, template bodies, SKILL.md outline, and validation status.

### skill-maintenance-translate

1. Convert the classified source skill into a hKask registry crate (manifest.yaml + .j2 templates).
2. Produce one .j2 file per classified step, mapping cognitive steps to KnowAct, workflow steps to WordAct or FlowDef, and guardrails to visibility, energy_cap, and constraints.
3. Map source state to .j2 contract input/output, user-confirmation gates to visibility, and domain references using the domain substitution table.
4. Mark any references with no hKask equivalent as `[unresolved: no hKask equivalent for <source_ref>]`.
5. Respond with a JSON object containing the manifest, templates, derived SKILL.md, and a translation summary detailing preserved, adapted, dropped, and unresolved elements.

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

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `skill-maintenance-validate.j2` | KnowAct | Validate skills against registry format and quality checks. Check manifest structure, .j2 frontmatter (template_type, contract, visibility, energy_cap). SKILL.md is validated as secondary companion.  |
| `skill-maintenance-build.j2` | KnowAct | Scaffold a new registry crate from a user description. Generate manifest.yaml with crate metadata, template entries, and lexicon_terms. Generate companion SKILL.md from the registry crate. Validate and confirm before writing.  |
| `skill-maintenance-translate.j2` | KnowAct | Forward translation: convert a classified source skill into a hKask registry crate (manifest.yaml + *.j2 templates). Map source elements to hKask equivalents, drop concepts with no equivalent, produce validated output with translation summary.  |
| `skill-maintenance-reverse.j2` | KnowAct | Reverse translation: generate a SKILL.md companion from a registry crate. Read manifest.yaml for crate metadata, read .j2 templates for methodology, produce a markdown companion suitable for the Zed coding agent.  |
| `skill-maintenance-prose.j2` | KnowAct | Prose-only derivation: synthesize the "When to Use" and "Instructions" sections of a SKILL.md from a registry crate, emitted as raw markdown. Used by `kask skill derive` alongside the mechanically-built skeleton (frontmatter, templates table, constraints) — the LLM only writes the prose that needs synthesis, not the structural parts copied from the registry.  |

## Constraints

- `skill-maintenance-validate.j2`: Public.
- `skill-maintenance-build.j2`: Public.
- `skill-maintenance-translate.j2`: Public.
- `skill-maintenance-reverse.j2`: Public.
- `skill-maintenance-prose.j2`: Public.
- Registry is authoritative — when this SKILL.md disagrees with registry templates, the registry wins.
