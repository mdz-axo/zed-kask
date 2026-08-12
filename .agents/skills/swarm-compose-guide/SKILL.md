---
name: swarm-compose-guide
visibility: public
description: "Agent/swarm composition authoring and validation aid. Given partial form inputs, renders guidance templates and returns either suggested completions for unfilled fields (action=suggest) or a validation verdict over supplied fields (action=validate)."
---


# Swarm Compose Guide

Agent/swarm composition authoring and validation aid. Given partial form inputs
(surface: agent|swarm, mode: abw|local, action: suggest|validate, plus the
author/compose fields), renders the `swarm-compose-guide.j2` Jinja2 guidance
template and returns either suggested completions for unfilled fields
(action=suggest) or a validation verdict over the supplied fields
(action=validate). Read-only authoring aid — no ledger debit, no consent.

The Jinja2 template (`swarm-intelligence/swarm-compose-guide.j2`) is the
canonical source for composition guidance (field definitions, ABW/Local
considerations, composition principles). It is shared with the
swarm-intelligence skill's template set. This skill makes it invocable as a
standalone cascade from the MCP server (`swarm_ai_assist`) via the
`SkillExecPort` path.

## When to Use

- The swarm panel's AI Assist button is clicked (action=suggest) — the cascade
  suggests completions for empty or partial author/compose form fields.
- The swarm panel's Validate button is clicked (action=validate) — the cascade
  checks well-formedness and surfaces issues before creating the agent or swarm.
- The swarm-intelligence DECIDE phase proposes an `author_agent` move and needs
  canonical authoring guidance for field definitions and backend constraints.

## Instructions

The cascade is a single-step `select` that renders the
`swarm-intelligence/swarm-compose-guide.j2` template with the operator's partial
inputs and the surface/mode/action selectors. The template encodes:

- Field definitions for the agent surface (name, agent_type, description,
  system_prompt) and the swarm surface (name, mission, agents).
- ABW (cloud catalogue) vs Local (filesystem) backend considerations, including
  slug rules, consent gating, cost models, and authoring workflows.
- Composition guidance: single-responsibility agents, swarm variety, clear
  inputs/outputs, concrete missions, transform coverage.
- Output instructions branched on action: suggest returns a JSON object of field
  completions; validate returns a JSON verdict (valid, issues, suggestions).

The form fields are serialized as a JSON object string and passed as the `task`
through the `SkillExecPort::execute_skill` seam. `AgentSkillExec` (zed side)
detects JSON-object tasks and merges their fields into the cascade context as
top-level template variables.

## Registry Templates

| Template | Type | Purpose |
|----------|------|--------|
| `swarm-intelligence/swarm-compose-guide.j2` | `KnowAct` | Agent/swarm composition authoring + validation guidance. Renders field definitions, ABW/Local considerations, and composition principles; returns JSON suggestions (action=suggest) or a validation verdict (action=validate). |

## Constraints

- Read-only authoring aid — no ledger debit, no consent, no ABW calls.
- The template uses the local `InferencePort` (one-shot LLM generate) in both
  modes; the `mode` field only tailors the guidance text.
- Conciseness is folded into the template's own instructions (focused,
  single-responsibility output). The caveman skill is a conceptual dependency
  for conciseness but is NOT run as a post-process step — it would corrupt the
  JSON suggestions object.
- Registry is authoritative — when this SKILL.md disagrees with registry
  templates, the registry wins.