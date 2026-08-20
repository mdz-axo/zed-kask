---
title: "ADR: Build-time embedded YAML/Jinja2 registry — superseded by body injection"
audience: [architects, developers, agents]
last_updated: 2026-08-20
version: "0.37.0"
status: "Superseded"
domain: "architecture"
mds_categories: [composition, lifecycle, trust]
---

# ADR: Build-time embedded YAML/Jinja2 registry — superseded by body injection

**Status:** Superseded. The build-time embedding model this ADR described is no
longer the skill-execution mechanism. Skill execution is upstream-Zed body
injection.

## Current reality

Skill execution is **upstream-Zed body injection**. `SkillTool::run`
(`crates/agent/src/tools/skill_tool.rs:266`) reads the `SKILL.md` body from disk
via `agent_skills::read_skill_body` and injects it into the model context via
`render_skill_envelope`. The model reads the body and follows the instructions.
PDCA loops are **model-coordinated**: the `SKILL.md` body describes convergence
criteria and the model self-iterates using the `lisp_eval` tool
(`hkask_lisp::eval_sandboxed_with_budget`) for deterministic checks and the
`render_template` tool for structured prompt scaffolding.

There is no `ManifestExecutor`, no `StepMachine`, no `FlowDef`, no
`BridgeManifestExecutor`, no `build.rs`, no `Registry`, no `TemplateRenderer`,
no `BudgetTracker`, no `ConvergenceTracker`, no `BundleManifest`, no `ExitKind`,
and no `kask/registry/manifests/` directory.

## What survives

- **`SKILL.md` companions** in `.agents/skills/<name>/SKILL.md`, discovered by
  `agent_skills` and injected by `SkillTool::run`.
- **Jinja2 templates** under `kask/registry/templates/` (62 template crates
  remain), rendered by the `render_template` tool (registered in
  `register_session`, not `add_default_tools`). The tool strips YAML frontmatter
  — the frontmatter's `contract:` and `[inference]` blocks are NOT processed;
  LLM parameters (temperature, thinking_budget) in the frontmatter have no
  effect. Path traversal protection via `canonicalize` + `starts_with` rejects a
  `template_ref` containing `..` that resolves outside the base path. The
  template base path is wired via `agent::set_template_base_path()` (OnceLock) in
  `main.rs` at startup (dev: `kask/registry/templates/`, prod:
  `{kask_data_dir}/skills/registry/templates/`); if unset, the tool returns an
  error rather than rendering from the wrong path.
- **`lisp_eval`** — a registered built-in tool (`add_default_tools`): a sandboxed
  Lisp interpreter with no I/O, no `eval`, no network, bounded by `max_steps`
  (default 100000) and `max_depth` (default 64).

## Why the build-time model was superseded

The build-time embedding model embedded all four artifact classes
(per-skill template manifests, FlowDef cascades, Jinja2 templates, FlowDef
sub-manifests) into the binary via `include_str!` and drove skill execution
through a `ManifestExecutor` cascade. That machinery is gone. Skill execution
now follows upstream Zed's progressive-disclosure pattern: only `name` and
`description` are preloaded into the system prompt, and the `SKILL.md` body loads
only when the `skill` tool is invoked. The Jinja2 template layer survives as a
prompt-scaffolding resource the model retrieves on demand via `render_template`,
not as a parallel representation of skill semantics.
