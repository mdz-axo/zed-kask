---
title: "Skill Registry — Reference"
audience: [developers, skill-authors, agents]
last_updated: 2026-09-24
version: "0.39.0"
status: "Active"
domain: "Core"
mds_categories: [domain, composition]
---

# Skill Registry

> **Execution model (verified 2026-08-28):** Skills execute via **upstream Zed body injection**.
> `SkillTool::run` (`crates/agent/src/tools/skill_tool.rs:167`) reads the `SKILL.md` body from disk
> and injects it into the agent's context via `render_skill_envelope`. The model reads the body
> and follows the instructions. The agent is the executor.
>
> **Two tools support skill execution:**
> - `lisp_eval` — sandboxed Lisp interpreter (`hkask_lisp::eval_sandboxed_with_budget`). No I/O,
>   no `eval`, no network. Bounded by `max_steps` (default 100000) and `max_depth` (default 1024)
>   (`kask/crates/hkask-lisp/src/hkask_lisp.rs:8`, call-site defaults at `:1677`). The model calls it when a SKILL.md instructs
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
> plus a markdown body of process instructions. **68 skills** are authored here: **54 ship** to every zed-kask user and **14 are developer-only** (`shipped: false`). **300 Jinja2 templates across 61
> template namespaces** remain under `kask/registry/templates/` for use by `render_template`; these
> are companion resources, not the source of truth for skill execution.

**Skill lifecycle:** A skill is activated when the agent invokes the `skill` tool with a skill
name. `SkillTool::run` resolves the skill directory, reads `SKILL.md`, and injects the body via
`render_skill_envelope`. The model reads the instructions and follows them — calling `lisp_eval`
for deterministic computation, `render_template` for structured prompt scaffolding, and MCP tools
for external capabilities.

**Composition law:** every skill's SKILL.md body carries its core process — including at least
one PDCA (Plan→Do→Check→Act) self-improvement loop with a clear improvement dimension (the
Check step's measurable signal, a threshold or convergence criterion, and a bound). Templates
are leaves: steps of that loop, rendered at the points the SKILL.md directs — never the
carrier of the loop itself.

---

## Registry counts (verified 2026-09-24)

| Surface | Count | Notes |
|---------|-------|-------|
| `SKILL.md` directories (`.agents/skills/*/`, repo root) | **68** | 54 shipped + 14 developer-only; filesystem count: `find .agents/skills -mindepth 2 -maxdepth 2 -name SKILL.md` |
| Template namespaces (`kask/registry/templates/*/`) | **61** (**300** `.j2` templates) | Companion Jinja2 resources for `render_template`; counts come directly from the current tree |

**The SKILL.md is the source of truth.** A skill is its `SKILL.md`. Template crates are
read-only resources the skill body may reference via `render_template`.

**Who a skill is for (operator ruling 2026-09-24).** Skills must be useful to the
human user of zed-kask. A skill used only to develop zed-kask itself declares
`shipped: false` in its frontmatter: `crates/agent_skills/build.rs` leaves it out of
the embedded payload, so installed builds never show it, and a zed-kask checkout
loads it as a project skill (visible only while zed-kask is the open project). A
developer-only skill cannot be `core: true`. Pinned by
`shipped_skill_seed_all_parse_without_errors` and
`development_shipped_skill_has_one_live_source`
(`crates/agent_skills/agent_skills.rs`).

---

## Shipped skills (54)

### Research, markets and forecasting

| Skill | Purpose |
|-------|---------|
| `company-research-deep` | Equity research deep pipeline. Sequential 16-step process converging on THESIS investment-grade verdict |
| `company-research-flash` | Equity research flash pipeline. Sequential 23-step process with early-exit gates converging on LENS verdict consistency |
| `portfolio-review` | Transaction-ledger portfolio performance review: seed prices from live quotes, TWR/MWR returns, Brinson-style attribution, durable review note |
| `superforecasting` | Calibrated probability forecasting (Tetlock's Good Judgment Project) |
| `scenario-planning` | Complete scenario project: Schwartz framing, forces and divergent 2x2 narratives with a quality gate and early-warning indicators, Tetlock quantification and propagation, Brier-scored resolution, Chermack assessment |
| `eqm` | Explanation Quality Markers: score forecast rationales against 60 EQMs via `market_score_rationale`, validate against realized outcomes (Brier), and improve a rationale in-session without changing its probability |
| `cmp-term-structure` | Constant-Maturity Prediction term structures: ladder, context, provenance-carrying indices, event-tree composition, contract-price coherence, equity-duration matching |
| `calibration-stewardship` | Prediction-market calibration loop maintenance: two-phase resolution scans, snapshot pairing, per-bucket Brier, reliability-tier demotion verification |
| `listening` | Apply the MAIA v3 listening template to an earnings-call transcript using a retrieve-cite-verify process |

### Media, writing and diagrams

| Skill | Purpose |
|-------|---------|
| `transcript-reel` | Recording → highlight reel over the educt layer system: transcribe, correct, highlight, EDL, render, export |
| `media-workflow` | Multi-tool media generation pipelines (product shots, stylized art, reaction GIFs, collages, memes, NFT derivatives) chaining media server tools in known-good sequences |
| `logo-builder` | Pragmatic logo design (Improvement Kata: Martin MVB → Bokhua gates → Peters iterative refinement) |
| `writing-style` | Compose or rewrite prose from curated style corpora and validate the result against measured style centroids |
| `sankey-flow` | Dynamic Sankey flow diagramming: classify domain, gather quantities, render Mermaid `sankey-beta` |
| `diataxis-diagram` | Generate Mermaid diagrams from code using Diataxis methodology |

### Decisions, research and evidence

| Skill | Purpose |
|-------|---------|
| `mcda` | Multi-Criteria Decision Analysis with compensation masking |
| `wardley-mapper` | Generic Wardley mapping: inventory components, classify evolution, map value chain, derive strategy |
| `hypothesis-framer` | Research question framing via FINER + PICO |
| `structured-extraction` | Extract structured data from unstructured text |
| `grounding-verify` | Verify factual claims in text against source data: extract claims, assign provenance, scan narrative, compute fact_score |
| `build-corpus-pipeline` | 10-stage corpus pipeline: convert → chunk → tag → embed → query → build_prompts → ingest_qa → assemble_dataset |

### Working with the agent

| Skill | Purpose |
|-------|---------|
| `algedonic-review` | Human-in-the-loop review with the operator and Curator: alert triage, then the gemba walk — the only place skills are evaluated |
| `therapy` | Memory therapy session — scan a memory DB (curator, replica/corpus, or swarm) for contradictions, fragmentation, and miscalibrated confidence; resolve, then reify lessons as skills/templates/rules |
| `adhd-mode` | Session-scoped output mode shaping responses for a reader with ADHD: next-action-first, numbered steps, state restated across turns, capped lists, deterministic pre-send gate (render_template + lisp_eval), optional caveman compression variant (absorbed 2026-09-09) |
| `grill-me` | Socratic questioning to stress-test understanding |
| `kata-coaching` | 5-question Coaching Kata dialogue |
| `product-manager` | The operator's side of the Division of Responsibilities: requirements as falsifiable outcome claims, spec provenance, acceptance criteria that can fail, ground-truth confirmation |
| `task-breakdown` | Convergent planning: vertical task slicing with acceptance criteria, checkpoints, and skill_match_query routing |
| `kanban-task-management` | Unified kanban task management across the full task lifecycle |
| `prompt-enhance` | General-purpose prompt enhancement: 7-type taxonomy routing with 3-tier effort knob |
| `swarm-intelligence` | Agent-swarm composition PDCA (SENSE → ORIENT → DECIDE → ACT → CHECK → CONVERGE) plus the swarm panel's agent/swarm authoring aid |
| `swarm-steering` | Focused local-swarm steering: codifies the execute-and-feed-back loop |
| `adapter-lifecycle` | Verifier-gated fine-tuning loop: rollout harness, verdict-bridged datasets, gated submit, A/B evaluation, feedback retrain |
| `lora-training` | LoRA/QLoRA training config and contract enforcement: 8-gate PEFT method selection, math/quant/data/harness audit |

### Coding in your project

| Skill | Purpose |
|-------|---------|
| `code-review` | Convergent code review of a change against its stated spec: scope → multi-axis perspectives → adjudicate → report → optional implement |
| `diagnose` | Disciplined diagnosis loop: reproduce → anchor → hypothesise → instrument → fix → regression-test |
| `tdd` | Test-driven development: RED → GREEN → REFACTOR loop |
| `bug-hunt` | Bug hunting expeditions against target crates using Weinberg, Beizer, Bach, Hendrickson methodologies |
| `refactor-architecture` | End-to-end architecture refactoring: discover friction, rank candidates, walk design tree, audit duplication, plan strangler-fig migration, verify integrity |
| `idiomatic-rust` | Type-driven Rust design through Graydon Hoare's principles |

### Disciplines the agent applies on its own

| Skill | Purpose |
|-------|---------|
| `coding-guidelines` | Enforce Karpathy's four coding principles: Think Before Coding, Simplicity First, Surgical Changes, Goal-Driven Execution |
| `deep-module` | Module design via Ousterhout's deletion test and interface minimalism |
| `essentialist` | Recursive eliminative interrogation (Exist → Surface → Contract) |
| `pragmatic-semantics` | Classify statements by certainty, constraint force, provenance |
| `pragmatic-cybernetics` | Feedback loops, variety engineering, system homeostasis |
| `falsifiability` | Eliminative inference: Popper falsifiability gate, Chamberlin multiple hypotheses, Platt strong inference, Pearl counterfactuals |
| `metacognition` | Improvement-Kata self-reflection: grasp, target, predict, experiment (including branching inquiry with delegation), measure the gap; predictions recorded for operator scoring |
| `gradient-hunter` | Find steep gradients between populated and unpopulated regions of a codebase/telemetry/test field |
| `lean-prover` | Machine-checked proof construction through Curry-Howard/de Bruijn/Carneiro lens. Sibling to falsifiability |
| `onto-anchor` | Resolve domain terms through the published-ontology fallback ladder before naming, classifying, or computing with them |
| `program-manager` | The agent's side of the Division of Responsibilities: recover the spec before building, design before coding, execute surgically, verify against a real definition of done |
| `goal-analysis` | Goal specification and completion verification |
| `kata-improvement` | 4-step Improvement Kata PDCA pattern (includes beginner_mode drills) |
| `verification-compression` | Compress a verification workflow without losing expectation coverage, falsifiers, failure visibility, provenance, or fault-detection signal; Lean-checked graph preservation |

## Developer-only skills (14, `shipped: false`)

Used to develop zed-kask itself; loaded only as project skills of this repository.

| Skill | Purpose |
|-------|---------|
| `create-skill` | Author or translate a skill: ontology research, PDCA derivation, scaffold under the artifact contract, prescreen, validate |
| `skill-maintenance` | Validate and audit existing skills; compare designs and file proposals for the algedonic review |
| `skill-logic-audit` | Goal- and callsite-grounded audit of `.j2` templates; files a comparison-backed proposal for the algedonic review |
| `skill-discovery` | Acquire NEW skills: detect capability gaps, search catalog, evaluate candidates, guide installation |
| `skill-router` | Route tasks to installed skills: ranked fit-scored recommendations + uncovered capability gap signals |
| `skill-bundler` | Compose multiple skills into a cohesive bundle |
| `self-improvement` | Unified self-induced update operator: nested PDCA + outer Improvement Kata across two pathways — Foundation Model (θ) and Scaffolding (Σ) |
| `gpa-evolution` | Genetic-Pareto evolutionary optimization over text artifacts: sample, reflect, mutate, recombine Pareto frontier |
| `gpui-bench` | Design, write, review, run, and interpret production-shaped GPUI Criterion benchmarks (renderer/task benches, responsiveness, hang regressions, before/after evidence) |
| `ui-layout-discipline` | Measured layout discipline for GPUI card/panel renderers |
| `kask-seam-audit` | Convergent multi-skill audit of the zed-kask Kask-Zed seam (`DIVERGENCE.md` is the current numbered-seam authority) |
| `upstream-rebase` | Manage upstream Zed rebases for zed-kask: per-D-seam-file strategy, mapped re-application, test-pin, DIVERGENCE.md update |
| `doc-update` | Realign the kask/docs tree with the code: condensation triage (<70 cap), ground-compare-recompose per docs-set, file:line citation gates, corpus-tool decision point |
| `improv` | Agent interaction grammar (Plussing, Yes And, Freestyling, Riffing) |

> **Filesystem reality (verified 2026-09-24):** `.agents/skills/` contains 68
> `SKILL.md` directories. Merged by operator decision 2026-09-24:
> `gemba-walk` into `algedonic-review`, `sequential-inquiry` into
> `metacognition`, `swarm-compose-guide` into `swarm-intelligence`,
> `scenario-builder` into `scenario-planning`, `eqm-improvement` into `eqm`;
> removed: `idiomatic-lisp`, `constraint-forces-recast`,
> `gradient-seeded-recombination`, `capabilities-reasoner`,
> `principle-constraints`. `kask/registry/templates/` contains 61 template
> namespaces holding 300 `.j2` files.
