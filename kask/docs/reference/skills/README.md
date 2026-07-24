---
title: "Skill, Template, and Bundle Registry — Reference"
audience: [developers, skill-authors, agents]
last_updated: 2026-07-24
version: "0.31.0"
status: "Active"
domain: "Core"
mds_categories: [domain, composition]
last-verified-against: "b922e9529"
---

# Skill, Template, and Bundle Registry

> **Layout (verified against the filesystem):** A skill is a **PDCA improving loop** composed of two artifacts:
> 1. a **FlowDef manifest** at `registry/manifests/<name>.yaml` — the steps, `convergence.threshold`, gas budget, and `loop` actions; this is what `ManifestExecutor` drives.
> 2. a **template crate** at `registry/templates/<name>/` — `manifest.yaml` (template metadata: ids, types, lexicon) plus the `*.j2` templates referenced by the FlowDef's `template_ref` values.
>
> The template crate is the **single source of truth** (P5.1). A **SKILL.md** companion in `.agents/skills/<name>/` is *derived* from the registry crate via the `skill-maintenance` skill (`skill-maintenance-reverse.j2`, LLM-driven) — it is not independently authored and is not required for runtime. Skills execute inside an agent's inference environment (REPL/chat); there is no standalone "run a skill" surface, by design.
>
> **Manifest category:** every FlowDef manifest carries a `manifest.category` field distinguishing agent skills from infrastructure that shares the `.yaml` form: `skill` (agent PDCA loop, bindable as an agent `process_manifest`), `qa-script` (run by `kask qa`), `runtime-config` (system bootstrap config), `daemon-process` (Regulation/Curator daemon, run directly — not agent-bound), `pipeline` (MCP-server/pipeline processes). `resolve_manifest` only binds `skill` manifests to agents; the audit counts only `skill`-category template crates as skills (non-skill template crates are health-checked but reported separately).

**Skill lifecycle:** Skills are PDCA (Plan-Do-Check-Act) loops with convergence thresholds, gas budgets, and `loop` actions; the cascade iterates until the convergence metric ≤ threshold or `max_iterations` is exhausted. Templates are one-shot prompt executions. The "kata bundle" is a conceptual composition of `kata-starter` + `kata-improvement` + `kata-coaching` realized by `KataEngine` routing — there is **no** `registry/bundles/kata/manifest.yaml` file; the three kata skills each have their own FlowDef manifest in `registry/manifests/`.

**Template types (Pattern A):** a triad of inference-invoked cognitive acts — `WordAct` (speech acts — "what to say"), `KnowAct` (metacognition — "how to think"), `FlowDef` (process — "what to do", `.yaml`) — plus `RenderAct`, a non-inference type for Jinja2 components that produce text via rendering (reference content, `{% macro %}` libraries, error views included via `{% include %}`/`{% from %}`) and are never sent to the LLM. The action is the rendering. See `crates/hkask-types/src/template_type.rs` and `crates/hkask-services-skill/src/audit.rs`.

---

## Open issues in this registry (2026-07-17)

- **SKILL.md derivation is not wired.** No `skill-translator` code or CLI command exists; the `skill-maintenance-reverse.j2` template is the only derivation path and must be invoked as a skill by an agent. Existing SKILL.md files may be hand-maintained (a P5.1 drift risk).
- **Count reconciliation:** the filesystem has 101 registry manifests (50 category=skill, 51 non-skill). 90 template crates under `registry/templates/`; 56 SKILL.md directories under `.agents/skills/`. Of the 56 SKILL.md directories, 6 have no FlowDef manifest (template-only or non-skill: `gpui-test`, `lint-creator`, `lora-training`, `qa-script-builder`, `skill-router`, `zed-cherry-pick`). Of those 6, three are catalogued here as `Template` type (`lora-training`, `qa-script-builder`, `skill-router`); the other three (`gpui-test`, `lint-creator`, `zed-cherry-pick`) are Zed-project-local skills not surfaced in this hKask registry catalog. The kata bundle is a registry manifest composing kata-coaching, kata-improvement, and kata-starter — not a separate `.agents/skills/` directory. Total catalogued: 55 (51 skills + 3 templates + 1 bundle).

---

## Guardrails (1 skill)

| Skill | Type | Purpose | Artifacts |
|-------|------|---------|----------|
| `coding-guidelines` | Skill | Enforce Karpathy's four coding principles: Think Before Coding, Simplicity First, Surgical Changes, Goal-Driven Execution | `registry/manifests/coding-guidelines.yaml` · `registry/templates/coding-guidelines/` |

---

## Core Development (11 skills)

| Skill | Type | Purpose | Artifacts |
|-------|------|---------|----------|
| `bug-hunt` | Skill | Bug hunting expeditions against target crates using Weinberg, Beizer, Bach, Hendrickson methodologies | `registry/manifests/bug-hunt.yaml` · `registry/templates/bug-hunt/` |
| `tdd` | Skill | Test-driven development: RED → GREEN → REFACTOR loop | `registry/manifests/tdd.yaml` · `registry/templates/tdd/` |
| `diagnose` | Skill | Disciplined diagnosis loop: reproduce → anchor → hypothesise → instrument → fix → regression-test | `registry/manifests/diagnose.yaml` · `registry/templates/diagnose/` |
| `deep-module` | Skill | Module design via Ousterhout's deletion test and interface minimalism (≤7 public functions) | `registry/manifests/deep-module.yaml` · `registry/templates/deep-module/` |
| `refactor-service-layer` | Skill | Extract shared service layer via strangler fig pattern | `registry/manifests/refactor-service-layer.yaml` · `registry/templates/refactor-service-layer/` |
| `improve-codebase-architecture` | Skill | Find deepening opportunities in codebases | `registry/manifests/improve-codebase-architecture.yaml` · `registry/templates/improve-codebase-architecture/` |
| `strangler-fig` | Skill | Incremental architectural migration via Fowler's Strangler Fig pattern | `registry/manifests/strangler-fig.yaml` · `registry/templates/strangler-fig/` |
| `idiomatic-rust` | Skill | Type-driven Rust design through Graydon Hoare's principles | `registry/manifests/idiomatic-rust.yaml` · `registry/templates/idiomatic-rust/` |
| `task-breakdown` | Skill | Convergent planning: vertical task slicing with acceptance criteria, checkpoints, and skill_match_query routing | `registry/manifests/task-breakdown.yaml` · `registry/templates/task-breakdown/` |
| `codegraph` | Skill | Code understanding: discover, map, and query the target codebase for goal-relevant context | `registry/manifests/codegraph.yaml` · `registry/templates/codegraph/` |
| `diataxis-diagram` | Skill | Generate Mermaid diagrams from code using Diataxis methodology | `registry/manifests/diataxis-diagram.yaml` · `registry/templates/diataxis-diagram/` |

---

## Reasoning & Analysis (10 skills)

| Skill | Type | Purpose | Artifacts |
|-------|------|---------|----------|
| `pragmatic-semantics` | Skill | Classify statements by certainty, constraint force, provenance | `registry/manifests/pragmatic-semantics.yaml` · `registry/templates/pragmatic-semantics/` |
| `pragmatic-cybernetics` | Skill | Feedback loops, variety engineering, system homeostasis | `registry/manifests/pragmatic-cybernetics.yaml` · `registry/templates/pragmatic-cybernetics/` |
| `pragmatic-laziness` | Skill | Find the path of least action through meaning-space | `registry/manifests/pragmatic-laziness.yaml` · `registry/templates/pragmatic-laziness/` |
| `essentialist` | Skill | Recursive eliminative interrogation (Exist → Surface → Contract) | `registry/manifests/essentialist.yaml` · `registry/templates/essentialist/` |
| `review` | Skill | Self-critique for contradictions, unsupported claims, logical gaps | `registry/manifests/review.yaml` · `registry/templates/review/` |
| `grill-me` | Skill | Socratic questioning to stress-test understanding | `registry/manifests/grill-me.yaml` · `registry/templates/grill-me/` |
| `zoom-out` | Skill | Broader context on unfamiliar code | `registry/manifests/zoom-out.yaml` · `registry/templates/zoom-out/` |
| `sequential-inquiry` | Skill | Dynamic chain-of-thought with automatic deep-dive delegation | `registry/manifests/sequential-inquiry.yaml` · `registry/templates/sequential-inquiry/` |
| `falsifiability` | Skill | Eliminative inference: Popper falsifiability gate, Chamberlin multiple hypotheses, Platt strong inference, Pearl counterfactuals | `registry/manifests/falsifiability.yaml` · `registry/templates/falsifiability/` |
| `metacognition` | Skill | Master self-reflection: decompose goals, assess progress, calibrate strategy, GEPA self-improvement | `registry/manifests/metacognition.yaml` · `registry/templates/metacognition/` |

---

## Kata & Coaching (4 skills + kata composition)

| Skill | Type | Purpose | Artifacts |
|-------|------|---------|----------|
| `kata` | Composition | Toyota Kata system — composes starter + improvement + coaching (realized by `KataEngine` routing; no standalone manifest file) | *(no file — routes to the three kata skills)* |
| `kata-coaching` | Skill | 5-question Coaching Kata dialogue | `registry/manifests/kata-coaching.yaml` · `registry/templates/kata-coaching/` |
| `kata-improvement` | Skill | 4-step Improvement Kata PDCA pattern | `registry/manifests/kata-improvement.yaml` · `registry/templates/kata-improvement/` |
| `kata-starter` | Skill | Foundational kata practice routines | `registry/manifests/kata-starter.yaml` · `registry/templates/kata-starter/` |
| `improv` | Skill | Agent interaction grammar (Plussing, Yes And, Freestyling, Riffing) | `registry/manifests/improv.yaml` · `registry/templates/improv/` |

---

## Meta & Maintenance (7 skills + 1 template)

| Skill | Type | Purpose | Artifacts |
|-------|------|---------|----------|
| `self-improvement` | Skill | Unified self-induced update operator (Ren et al. 2026, arXiv:2607.13104): nested PDCA + outer Improvement Kata across two pathways — Foundation Model (θ) and Scaffolding (Σ) — driven by intrinsic generative demos, intrinsic evaluative feedback, and extrinsic exploratory experience | `registry/manifests/self-improvement.yaml` · `registry/templates/self-improvement/` |
| `skill-maintenance` | Skill | Audit skill architecture for staleness, coverage gaps; also derives SKILL.md from registry crates (reverse-translation) | `registry/manifests/skill-maintenance.yaml` · `registry/templates/skill-maintenance/` |
| `skill-logic-audit` | Skill | Audit .j2 template logic against stated goals | `registry/manifests/skill-logic-audit.yaml` · `registry/templates/skill-logic-audit/` |
| `skill-bundler` | Skill | Compose multiple skills into a cohesive bundle | `registry/manifests/skill-bundler.yaml` · `registry/templates/skill-bundler/` |
| `handoff` | Skill | Session handoff — capture what was done, what remains | `registry/manifests/handoff.yaml` · `registry/templates/handoff/` |
| `skill-discovery` | Skill | Acquire NEW skills: detect capability gaps, search catalog, evaluate candidates, guide installation | `registry/manifests/skill-discovery.yaml` · `registry/templates/skill-discovery/` |
| `skill-router` | Template | Route tasks to installed skills: ranked fit-scored recommendations + uncovered capability gap signals. Stateless `KnowAct` matching service invoked by the orchestrator and by process-skill templates (not a PDCA loop; cannot bind as `process_manifest`) | `registry/templates/skill-router/manifest.yaml` (no FlowDef manifest) · `registry/templates/skill-router/` |
| `gpa-evolution` | Skill | Genetic-Pareto evolutionary optimization over text artifacts: sample, reflect, mutate, recombine Pareto frontier | `registry/manifests/gpa-evolution.yaml` · `registry/templates/gpa-evolution/` |

---

## Security & Posture (4 skills)

| Skill | Type | Purpose | Artifacts |
|-------|------|---------|----------|
| `kali-audit` | Skill | Convergent security review: OWASP LLM Top 10, MITRE ATLAS, NIST SSDF against code, templates, manifests, MCP surfaces, LLM I/O | `registry/manifests/kali-audit.yaml` · `registry/templates/kali-audit/` |
| `supply-chain-sentinel` | Skill | Dependency and supply chain audit: version pinning, registry verification, license conflicts, unmaintained indicators | `registry/manifests/supply-chain-sentinel.yaml` · `registry/templates/supply-chain-sentinel/` |
| `runtime-posture-monitor` | Skill | Runtime security posture: observes Regulation telemetry for endpoint abuse, bot traffic, LLM usage anomalies | `registry/manifests/runtime-posture-monitor.yaml` · `registry/templates/runtime-posture-monitor/` |
| `attack-taxonomy-mapper` | Skill | Maps supply chain findings to OSC&R attack taxonomy; consumes supply-chain-sentinel and kali-audit findings | `registry/manifests/attack-taxonomy-mapper.yaml` · `registry/templates/attack-taxonomy-mapper/` |

---

## Specialized (13 skills + 2 templates)

| Skill | Type | Purpose | Artifacts |
|-------|------|---------|----------|
| `superforecasting` | Skill | Calibrated probability forecasting (Tetlock's Good Judgment Project) | `registry/manifests/superforecasting.yaml` · `registry/templates/superforecasting/` |
| `mcda` | Skill | Multi-Criteria Decision Analysis with compensation masking | `registry/manifests/mcda.yaml` · `registry/templates/mcda/` |
| `scenario-builder` | Skill | Schwartz scenario planning with STEEP analysis | `registry/manifests/scenario-builder.yaml` · `registry/templates/scenario-builder/` |
| `hypothesis-framer` | Skill | Research question framing via FINER + PICO | `registry/manifests/hypothesis-framer.yaml` · `registry/templates/hypothesis-framer/` |
| `adversarial-red-team` | Skill | Adversarial robustness testing with ATLAS/GARAK taxonomy | `registry/manifests/adversarial-red-team.yaml` · `registry/templates/adversarial-red-team/` |
| `goal-analysis` | Skill | Goal specification and completion verification | `registry/manifests/goal-analysis.yaml` · `registry/templates/goal-analysis/` |
| `magna-carta-verifier` | Skill | Verify Magna Carta principles enforcement | `registry/manifests/magna-carta-verifier.yaml` · `registry/templates/magna-carta-verifier/` |
| `structured-extraction` | Skill | Extract structured data from unstructured text | `registry/manifests/structured-extraction.yaml` · `registry/templates/structured-extraction/` |
| `caveman` | Skill | Multi-mode text compression | `registry/manifests/caveman.yaml` · `registry/templates/caveman/` |
| `self-critique-revision` | Skill | Iterative self-critique and revision cycle | `registry/manifests/self-critique-revision.yaml` · `registry/templates/self-critique-revision/` |
| `logo-builder` | Skill | Pragmatic logo design (Improvement Kata: Martin MVB → Bokhua gates → Peters iterative refinement) | `registry/manifests/logo-builder.yaml` · `registry/templates/logo-builder/` |
| `media-workflow` | Skill | Multi-step Fal.ai media pipeline composition and execution (Improvement Kata) | `registry/manifests/media-workflow.yaml` · `registry/templates/media-workflow/` |
| `qa-script-builder` | Template | Design autonomous QA pipeline manifests (one-shot, not PDCA) | `registry/templates/qa-script-builder/manifest.yaml` (no FlowDef manifest) |
| `semantic-graph-audit` | Skill | Domain-agnostic semantic dependency graph analysis | `registry/manifests/semantic-graph-audit.yaml` · `registry/templates/semantic-graph-audit/` |
| `wardley-mapper` | Skill | Generic Wardley mapping: inventory components, classify evolution, map value chain, derive strategy | `registry/manifests/wardley-mapper.yaml` · `registry/templates/wardley-mapper/` |
| `lora-training` | Template | LoRA/QLoRA training config and contract enforcement: 8-gate PEFT method selection, math/quant/data/harness audit. Stateless `KnowAct` templates (advisory; not a PDCA loop, cannot bind as `process_manifest`) | `registry/templates/lora-training/manifest.yaml` (no FlowDef manifest) · `registry/templates/lora-training/` |

---

## Summary

| Category | Count | Types |
|----------|-------|-------|
| Guardrails | 1 | Skill |
| Core Development | 11 | Skills |
| Reasoning & Analysis | 10 | Skills |
| Kata & Coaching | 4 skills + 1 composition | Skills + Composition |
| Meta & Maintenance | 7 skills + 1 template | Skills + Template |
| Security & Posture | 4 | Skills |
| Specialized | 13 skills + 2 templates | Skills + Template |
| **Catalogued here** | **51 skills + 3 templates + 1 bundle** | **55 capabilities** |

> **Filesystem reality:** `registry/templates/` contains 90 template directories; `registry/manifests/` contains 101 FlowDef manifests (50 category=skill, 51 non-skill: 21 pipeline, 11 infrastructure, 10 qa-script, 5 runtime-config, 3 daemon-process, 1 registry). `.agents/skills/` contains 56 SKILL.md directories (50 with a corresponding skill-category FlowDef manifest, 6 template-only or non-skill: `gpui-test`, `lint-creator`, `lora-training`, `qa-script-builder`, `skill-router`, `zed-cherry-pick`). Of those 6, three are catalogued here as `Template` type (`lora-training`, `qa-script-builder`, `skill-router`); the other three (`gpui-test`, `lint-creator`, `zed-cherry-pick`) are Zed-project-local skills not surfaced in this hKask registry catalog. The kata bundle is a registry manifest composing kata-coaching, kata-improvement, and kata-starter — not a separate `.agents/skills/` directory.