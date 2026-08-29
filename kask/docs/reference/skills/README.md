---
title: "Skill Registry — Reference"
audience: [developers, skill-authors, agents]
last_updated: 2026-08-24
version: "0.39.0"
status: "Active"
domain: "Core"
mds_categories: [domain, composition]
---

# Skill Registry

> **Execution model (verified 2026-08-24):** Skills execute via **upstream Zed body injection**.
> `SkillTool::run` (`crates/agent/src/tools/skill_tool.rs:167`) reads the `SKILL.md` body from disk
> and injects it into the agent's context via `render_skill_envelope`. The model reads the body
> and follows the instructions. The agent is the executor.
>
> **Two tools support skill execution:**
> - `lisp_eval` — sandboxed Lisp interpreter (`hkask_lisp::eval_sandboxed_with_budget`). No I/O,
>   no `eval`, no network. Bounded by `max_steps` (default 100000) and `max_depth` (default 64)
>   (`kask/crates/hkask-lisp/src/lib.rs:8`). The model calls it when a SKILL.md instructs
>   deterministic computation (convergence signals, invariant checks, scoring).
> - `render_template` — renders Jinja2 templates from `kask/registry/templates/` using `minijinja`.
>   Strips YAML frontmatter. Path traversal protection via `canonicalize` + `starts_with` check.
>   Template base path wired via `agent::set_template_base_path()` (OnceLock) in `main.rs` at startup.
>
> **PDCA loops are model-coordinated, not machine-enforced.** The SKILL.md body describes
> convergence criteria; the model self-iterates using `lisp_eval` for deterministic checks and
> `render_template` for structured prompt scaffolding. There is no runtime that drives the loop.
>
> **Layout:** A skill is a directory under `.agents/skills/<name>/` (repo root, not under `kask/`)
> containing a `SKILL.md` file with YAML frontmatter (`name`, `description`, and optional metadata)
> plus a markdown body of process instructions. 65 skills ship. 61 template crates remain under
> `kask/registry/templates/` for use by `render_template` — these are companion resources, not the
> source of truth for skill execution.

**Skill lifecycle:** A skill is activated when the agent invokes the `skill` tool with a skill
name. `SkillTool::run` resolves the skill directory, reads `SKILL.md`, and injects the body via
`render_skill_envelope`. The model reads the instructions and follows them — calling `lisp_eval`
for deterministic computation, `render_template` for structured prompt scaffolding, and MCP tools
for external capabilities. Convergence is the model's judgment, optionally checked by `lisp_eval`.

---

## Registry counts (verified 2026-08-24)

| Surface | Count | Notes |
|---------|-------|-------|
| SKILL.md directories (`.agents/skills/*/`, repo root) | **65** | Every directory contains a `SKILL.md` |
| Template crates (`kask/registry/templates/*/`) | **61** | Companion Jinja2 resources for `render_template` |

**The SKILL.md is the source of truth.** A skill is its `SKILL.md`. Template crates are
read-only resources the skill body may reference via `render_template`.

---

## Guardrails (1 skill)

| Skill | Purpose |
|-------|---------|
| `coding-guidelines` | Enforce Karpathy's four coding principles: Think Before Coding, Simplicity First, Surgical Changes, Goal-Driven Execution |

---

## Core Development (12 skills)

| Skill | Purpose |
|-------|---------|
| `bug-hunt` | Bug hunting expeditions against target crates using Weinberg, Beizer, Bach, Hendrickson methodologies |
| `tdd` | Test-driven development: RED → GREEN → REFACTOR loop |
| `diagnose` | Disciplined diagnosis loop: reproduce → anchor → hypothesise → instrument → fix → regression-test |
| `code-review` | Convergent code review of a change against its stated spec: scope → multi-axis perspectives → adjudicate → report → optional implement |
| `deep-module` | Module design via Ousterhout's deletion test and interface minimalism |
| `refactor-architecture` | End-to-end architecture refactoring: discover friction, rank candidates, walk design tree, audit duplication, plan strangler-fig migration, verify integrity |
| `idiomatic-rust` | Type-driven Rust design through Graydon Hoare's principles |
| `idiomatic-lisp` | Idiomatic Lisp design through McCarthy/Sussman/Graham principles (homoiconicity, metacircularity, data-as-program) with REPL evaluation as the extrinsic oracle |
| `task-breakdown` | Convergent planning: vertical task slicing with acceptance criteria, checkpoints, and skill_match_query routing |
| `diataxis-diagram` | Generate Mermaid diagrams from code using Diataxis methodology |
| `lisp-scaffold-reasoning` | Reference skill demonstrating deterministic Lisp scaffolding of LLM probabilistic reasoning |
| `kanban-task-management` | Unified kanban task management across the full task lifecycle |

---

## Reasoning & Analysis (10 skills)

| Skill | Purpose |
|-------|---------|
| `pragmatic-semantics` | Classify statements by certainty, constraint force, provenance |
| `pragmatic-cybernetics` | Feedback loops, variety engineering, system homeostasis |
| `essentialist` | Recursive eliminative interrogation (Exist → Surface → Contract) |
| `grill-me` | Socratic questioning to stress-test understanding |
| `sequential-inquiry` | Dynamic chain-of-thought with automatic deep-dive delegation |
| `falsifiability` | Eliminative inference: Popper falsifiability gate, Chamberlin multiple hypotheses, Platt strong inference, Pearl counterfactuals |
| `lean-prover` | Machine-checked proof construction through Curry-Howard/de Bruijn/Carneiro lens. Sibling to falsifiability |
| `capabilities-reasoner` | Reason about a system's capabilities against a typed registry with floor/ceiling/maturity-gate limits |
| `metacognition` | Master self-reflection: decompose goals, assess progress, calibrate strategy, GEPA self-improvement |
| `gradient-hunter` | Find steep gradients between populated and unpopulated regions of a codebase/telemetry/test field |

---

## Kata & Coaching (3 skills)

| Skill | Purpose |
|-------|---------|
| `kata-coaching` | 5-question Coaching Kata dialogue |
| `kata-improvement` | 4-step Improvement Kata PDCA pattern (includes beginner_mode drills) |
| `improv` | Agent interaction grammar (Plussing, Yes And, Freestyling, Riffing) |

---

## Meta & Maintenance (7 skills)

| Skill | Purpose |
|-------|---------|
| `self-improvement` | Unified self-induced update operator: nested PDCA + outer Improvement Kata across two pathways — Foundation Model (θ) and Scaffolding (Σ) |
| `skill-maintenance` | Audit skill architecture for staleness, coverage gaps; validate .j2 template logic against stated goals |
| `skill-bundler` | Compose multiple skills into a cohesive bundle |
| `skill-discovery` | Acquire NEW skills: detect capability gaps, search catalog, evaluate candidates, guide installation |
| `skill-router` | Route tasks to installed skills: ranked fit-scored recommendations + uncovered capability gap signals |
| `gpa-evolution` | Genetic-Pareto evolutionary optimization over text artifacts: sample, reflect, mutate, recombine Pareto frontier |
| `create-skill` | Convergent kask-native skill creation with ontological grounding |
| `skill-logic-audit` | Bounded dual-layer logic audit of .j2 templates and SKILL.md files against their stated goals |

---

## Specialized (15 skills)

| Skill | Purpose |
|-------|---------|
| `superforecasting` | Calibrated probability forecasting (Tetlock's Good Judgment Project) |
| `mcda` | Multi-Criteria Decision Analysis with compensation masking |
| `scenario-builder` | Schwartz scenario planning with STEEP analysis |
| `hypothesis-framer` | Research question framing via FINER + PICO |
| `goal-analysis` | Goal specification and completion verification |
| `structured-extraction` | Extract structured data from unstructured text |
| `caveman` | Multi-mode text compression (TTbS stage in stt-tts pipeline) |
| `logo-builder` | Pragmatic logo design (Improvement Kata: Martin MVB → Bokhua gates → Peters iterative refinement) |
| `wardley-mapper` | Generic Wardley mapping: inventory components, classify evolution, map value chain, derive strategy |
| `lora-training` | LoRA/QLoRA training config and contract enforcement: 8-gate PEFT method selection, math/quant/data/harness audit |
| `prompt-enhance` | General-purpose prompt enhancement: 7-type taxonomy routing with 3-tier effort knob |
| `sankey-flow` | Dynamic Sankey flow diagramming: classify domain, gather quantities, render Mermaid `sankey-beta` |
| `swarm-intelligence` | ABW agent-swarm composition PDCA: SENSE → ORIENT → DECIDE → ACT → CHECK → CONVERGE |
| `swarm-steering` | Focused local-swarm steering: codifies the execute-and-feed-back loop |
| `ui-layout-discipline` | Measured layout discipline for GPUI card/panel renderers |

---

## Research & Corpus (5 skills)

| Skill | Purpose |
|-------|---------|
| `company-research-deep` | Equity research deep pipeline. Sequential 16-step process converging on THESIS investment-grade verdict |
| `company-research-flash` | Equity research flash pipeline. Sequential 23-step process with early-exit gates converging on LENS verdict consistency |
| `listening` | Apply the MAIA v3 listening template to an earnings-call transcript using a retrieve-cite-verify process |
| `build-corpus-pipeline` | 10-stage corpus pipeline: convert → chunk → tag → embed → query → build_prompts → ingest_qa → assemble_dataset |
| `grounding-verify` | Verify factual claims in text against source data: extract claims, assign provenance, scan narrative, compute fact_score |

---

## Cross-Cutting & Audit (9 skills)

| Skill | Purpose |
|-------|---------|
| `algedonic-review` | Human-in-the-loop review and triage of the algedonic alert backlog |
| `gemba-walk` | Human-in-the-loop guided review of the cybernetic regulation system |
| `constraint-forces-recast` | Interdisciplinary concept generation via minimal-satisfiability projection |
| `gradient-seeded-recombination` | Find where to apply constraint-forces recast: inventory ontologies, build prior, map recombination field, detect gradients, select seeds |
| `principle-constraints` | Compiles a stated principle into checkable, code-path-anchored constraints with named falsifiers |
| `kask-seam-audit` | Convergent multi-skill audit of the zed-kask Kask-Zed seam (DIVERGENCE.md D1–D32) |
| `swarm-compose-guide` | Agent/swarm composition authoring aid: renders guidance templates, returns suggested completions or validation verdict |
| `upstream-rebase` | Manage upstream Zed rebases for zed-kask: per-D-seam-file strategy, mapped re-application, test-pin, DIVERGENCE.md update |

---

## Summary

| Category | Count |
|----------|-------|
| Guardrails | 1 |
| Core Development | 12 |
| Reasoning & Analysis | 10 |
| Kata & Coaching | 3 |
| Meta & Maintenance | 8 |
| Specialized | 15 |
| Research & Corpus | 5 |
| Cross-Cutting & Audit | 8 |
| **Total** | **65** |

> **Filesystem reality (verified 2026-08-28):** `.agents/skills/` (repo root) contains 65 SKILL.md
> directories. `kask/registry/templates/` contains 61 template crates (companion Jinja2 resources
> for `render_template`).
