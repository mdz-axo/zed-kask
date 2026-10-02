---
title: "Skill Registry — Reference"
audience: [developers, skill-authors, agents]
last_updated: 2026-09-29
version: "0.40.0"
status: "Active"
domain: "Core"
mds_categories: [domain, composition]
---

# Skill Registry

> **Execution model (verified 2026-08-28):** Skills execute via **upstream Zed body injection**.
> `SkillTool::run` (`crates/agent/src/tools/skill_tool.rs:194`) reads the `SKILL.md` body from disk
> and injects it into the agent's context via `render_skill_envelope`. The model reads the body
> and follows the instructions. The agent is the executor.
>
> **Two tools support skill execution:**
> - `lisp_eval` — sandboxed Lisp interpreter (`hkask_lisp::eval_sandboxed_with_budget`). No I/O,
>   no `eval`, no network. Bounded by `max_steps` (default 100000) and `max_depth` (default 1024)
>   (`kask/crates/hkask-lisp/src/hkask_lisp.rs:8`, call-site defaults at `:1733-1734`). The model calls it when a SKILL.md instructs
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
> plus a markdown body of process instructions. **60 skills** are authored here and available in every zed-kask install. **274 Jinja2 templates across 56
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

## Registry counts (verified 2026-09-30)

| Surface | Count | Notes |
|---------|-------|-------|
| `SKILL.md` directories (`.agents/skills/*/`, repo root) | **60** | filesystem count: `find .agents/skills -mindepth 2 -maxdepth 2 -name SKILL.md` |
| Template namespaces (`kask/registry/templates/*/`) | **56** (**274** `.j2` templates) | Companion Jinja2 resources for `render_template`; counts come directly from the current tree |

**The SKILL.md is the source of truth.** A skill is its `SKILL.md`. Template crates are
read-only resources the skill body may reference via `render_template`.

**Skills are evolving drafts (operator ruling 2026-09-26).** Every skill in `.agents/skills/`
is available to every install and every user. Skills and templates have no release state,
draft flag or developer-only tier: they change weekly or monthly through measured proposals,
not releases. Release vocabulary belongs to the Rust code, whose cadence is quarterly or
annual. The embedded seed payload equals the authored tree (pinned in
`crates/agent_skills/agent_skills.rs`).

---

## Skills (60)

### Research, markets and forecasting

| Skill | Purpose |
|-------|---------|
| `company-research-deep` | Equity research deep pipeline. Nine analytical perspectives run as dependency waves over one extract-once evidence digest; every load-bearing claim is checked against retained sources before publication, and the report is never labelled verified or investment grade |
| `company-research-flash` | Equity research flash pipeline. Sequential 23-step process with early-exit gates converging on LENS verdict consistency |
| `portfolio-review` | Transaction-ledger portfolio performance review: seed prices from live quotes, TWR/MWR returns, Brinson-style attribution, durable review note |
| `superforecasting` | Calibrated probability forecasting (Tetlock's Good Judgment Project) |
| `scenario-planning` | Complete scenario project: Schwartz framing, forces and divergent 2x2 narratives with a quality gate and early-warning indicators, Tetlock quantification and propagation, Brier-scored resolution, Chermack assessment |
| `eqm` | Explanation Quality Markers: score forecast rationales against the predictive 12 of the 60 EQMs via `market_score_rationale`, validate against realized outcomes (Brier), and improve a rationale in-session without changing its probability |
| `cmp-term-structure` | Constant-Maturity Prediction term structures: ladder, context, provenance-carrying indices, event-tree composition, contract-price coherence, equity-duration matching |
| `listening` | Apply the MAIA v3 listening template to an earnings-call transcript using a retrieve-cite-verify process |

### Media, writing and diagrams

| Skill | Purpose |
|-------|---------|
| `transcript-reel` | Recording → highlight reel over the educt layer system: transcribe, correct, highlight, EDL, render, export |
| `media-workflow` | Multi-tool media generation pipelines (product shots, stylized art, reaction GIFs, collages, memes, NFT derivatives, and the Logo pipeline — Martin MVB → Bokhua gates → operator-chosen refinement) chaining media server tools in known-good sequences |
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
| `product-manager` | The operator's side of the Division of Responsibilities: requirements as falsifiable outcome claims, spec provenance, acceptance criteria that can fail, ground-truth confirmation |
| `task-breakdown` | Convergent planning: vertical task slicing with acceptance criteria, checkpoints, and skill_match_query routing |
| `region-routing` | Route task steps across the syntax-semantic x deterministic-probabilistic 2x2 regions: named tools per step, a deterministic gate per generating step, role checkpoints at semantic and dependency boundaries |
| `kanban-task-management` | Unified kanban task management across the full task lifecycle |
| `prompt-enhance` | General-purpose prompt enhancement: 7-type taxonomy routing with 3-tier effort knob |
| `local-research-swarm` | Coordinate a project-sized, source-grounded research effort across a local agent roster, with a kanban board for work state and scoped A2A handoffs |
| `swarm-intelligence` | Agent-swarm composition PDCA (SENSE → ORIENT → DECIDE → ACT → CHECK → CONVERGE), its receipt-checked local steering loop, and the swarm panel's agent/swarm authoring aid |
| `lora-training` | LoRA/QLoRA training config and contract enforcement: 8-gate PEFT method selection, 19-gate phase-aware audit (math/quantization/data/forgetting/harness/runtime/persistence) |

### Coding in your project

| Skill | Purpose |
|-------|---------|
| `code-review` | Convergent code review of a change against its stated spec: scope → multi-axis perspectives → adjudicate → report → optional implement |
| `diagnose` | Disciplined diagnosis loop: anchor → reproduce → hypothesise → instrument → fix → regression-test |
| `tdd` | Test-driven development: red → green, one vertical slice at a time, tests only at agreed seams; refactoring is not in the loop (code-review owns it) |
| `bug-hunt` | Bug hunting expeditions against target crates using Weinberg, Beizer, Bach, Hendrickson methodologies |
| `refactor-architecture` | End-to-end architecture refactoring: discover friction, rank candidates, walk design tree, audit duplication, plan strangler-fig migration, verify integrity |
| `idiomatic-rust` | Type-driven Rust design through Graydon Hoare's principles |

### Disciplines the agent applies on its own

| Skill | Purpose |
|-------|---------|
| `coding-guidelines` | Enforce Karpathy's four coding principles: Think Before Coding, Simplicity First, Surgical Changes, Goal-Driven Execution |
| `deep-module` | Module design via Ousterhout's depth criteria (kask-operationalized deletion test, depth score, ≤7 surface cap) |
| `essentialist` | Recursive eliminative interrogation (Exist → Surface → Contract) |
| `pragmatic-semantics` | Classify statements by certainty, constraint force, provenance; route computation steps to deterministic or probabilistic machines (D/P labelling, P8.4) |
| `pragmatic-cybernetics` | Feedback loops, variety engineering, system homeostasis |
| `falsifiability` | Eliminative inference: Popper falsifiability gate, Chamberlin multiple hypotheses, Platt strong inference, Pearl counterfactuals |
| `metacognition` | Improvement-Kata self-reflection: grasp, target, predict, experiment (including branching inquiry with delegation), measure the gap; predictions recorded for operator scoring |
| `gradient-hunter` | Find steep gradients between populated and unpopulated regions of a codebase/telemetry/test field |
| `lean-prover` | Machine-checked proof construction through Curry-Howard/de Bruijn/Carneiro lens. Sibling to falsifiability |
| `onto-anchor` | Resolve domain terms through the published-ontology fallback ladder before naming, classifying, or computing with them |
| `program-manager` | The agent's side of the Division of Responsibilities: recover the spec before building, design before coding, execute surgically, verify against a real definition of done |
| `kata-improvement` | 4-step Improvement Kata PDCA pattern, the five-question Coaching Kata (coach role), and beginner_mode drills |
| `verification-compression` | Compress a verification workflow without losing expectation coverage, falsifiers, failure visibility, provenance, or fault-detection signal; Lean-checked graph preservation |

### Skill authoring and zed-kask development

| Skill | Purpose |
|-------|---------|
| `create-skill` | Author or translate a skill: ontology research, PDCA derivation, scaffold under the artifact contract, prescreen, validate |
| `skill-maintenance` | Validate and audit existing skills, including the template-logic audit of `.j2` goals and callsites and the prescreen's body-side D/P labelling presence check (P8.4); compare designs and file proposals for the algedonic review |
| `skill-discovery` | Route tasks to installed skills (fit-scored recommendations), detect capability gaps, evaluate candidates before installation |
| `skill-bundler` | Merge peer-level skill outputs into one grounded unified report (per-skill summaries, cross-skill insights, explicit conflicts, one bounded correction) |
| `self-improvement` | Unified self-induced update operator: nested PDCA + outer Improvement Kata across two pathways — Foundation Model (θ) and Scaffolding (Σ), including the GEPA prompt-evolution sub-loop |
| `gpui-bench` | Design, write, review, run, and interpret production-shaped GPUI Criterion benchmarks (renderer/task benches, responsiveness, hang regressions, before/after evidence) |
| `kask-seam-audit` | Convergent multi-skill audit of the zed-kask Kask-Zed seam (`DIVERGENCE.md` is the current numbered-seam authority); its measured-layout loop also runs standalone for GPUI cards and panels |
| `upstream-rebase` | Manage upstream Zed rebases for zed-kask: per-D-seam-file strategy, mapped re-application, test-pin, DIVERGENCE.md update |
| `doc-update` | Realign the kask/docs tree with the code: condensation triage (<75 cap, working target 60), ground-compare-recompose per docs-set, file:line citation gates, corpus-tool decision point |
| `improv` | Agent interaction grammar (Plussing, Yes And, Freestyling, Riffing) |

> **Filesystem reality (verified 2026-10-01):** `.agents/skills/` contains 60
> `SKILL.md` directories. Merged by operator decision 2026-09-24:
> `gemba-walk` into `algedonic-review`, `skill-router` into `skill-discovery`
> (route phase), `sequential-inquiry` into
> `metacognition`, `swarm-compose-guide` into `swarm-intelligence`,
> `scenario-builder` into `scenario-planning`, `eqm-improvement` into `eqm`;
> removed: `idiomatic-lisp`, `constraint-forces-recast`,
> `gradient-seeded-recombination`, `capabilities-reasoner`,
> `principle-constraints`, `goal-analysis` (its judge moved to
> `company-research/thesis-judge`, removed 2026-09-27 with the deep pipeline's
> gate machinery). Folded 2026-09-26: `adapter-lifecycle` into
> `self-improvement` (Fine-tuning run), `calibration-stewardship` into
> `superforecasting` (Market-prior calibration check). Folded 2026-09-28
> (commit `ebcd901c80`): `kata-coaching` into `kata-improvement` — the five
> coaching templates moved to `kask/registry/templates/kata-improvement/`.
> `kask/registry/templates/` contains 56
> template namespaces holding 274 `.j2` and 2 `.jinja` files.
