---
title: "Skill Registry — Reference"
audience: [developers, skill-authors, agents]
last_updated: 2026-10-30
version: "0.45.0"
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
> - `lisp_eval` — sandboxed Lisp for skill-directed deterministic checks; no I/O/`eval`/network.
>   Native background decode/evaluation uses `eval_sandboxed_with_cancellation`, one monotonic
>   flag, mandatory teardown, cleanup join and publication/weak pending-owner guards; raw/pure
>   APIs are unchanged (`kask/crates/hkask-lisp/src/hkask_lisp.rs:2505@eval_sandboxed_with_cancellation`;
>   `crates/agent/src/tools/lisp_eval_tool.rs:374@LispEvalTool::run`). Defaults remain 100000 steps /
>   depth 1024 (`lisp_eval_tool.rs:291@default_max_steps`, `:295@default_max_depth`). Titles read
>   form only; copying/display stay foreground. [Bounds and nonpreemptible phases](../lisp-eval-dialect.md#numeric-admission-and-limits)
>   are not hard time/heap/concurrency guarantees. No host/IPC lisp surface exists —
>   decode/evaluation runs in-process (the public shared `evaluate_lisp_controlled`,
>   `lisp_eval_tool.rs:487@evaluate_lisp_controlled`).
>   H8 direct/routed corpus queries share one body; only Lisp options are offloaded and
>   observe request-context cancellation, leaving downstream inference/retrieval/generation alone
>   (`kask/mcp-servers/hkask-mcp-corpus/src/tools/storage.rs:108-243`). Explicit rmcp cancel
>   reaches the worker; plain EOF does not immediately signal it (tested context cancellation
>   follows service-owner teardown after drain). No immediate disconnect or shutdown-worker-join promise.
>   H4C measurements remain deferred; H6 native Stop validation was not performed;
>   installed live build `1c247be309` predates H7/H8. No new runtime/service/dependency/cache/configuration.
> - `render_template` — renders Jinja2 templates from `kask/registry/templates/` using `minijinja`.
>   Strips the `[inference]`-keyed header. Path traversal protection via `canonicalize` + `starts_with` check.
>   Template base path wired via `agent::set_template_base_path()` (OnceLock) in `main.rs` at startup.
>
> **PDCA loops are model-coordinated, not machine-enforced.** The SKILL.md body describes
> convergence criteria; the model self-iterates using `lisp_eval` for deterministic checks and
> `render_template` for structured prompt scaffolding. There is no runtime that drives the loop.
>
> **Layout:** A skill is a directory under `.agents/skills/<name>/` (repo root, not under `kask/`)
> containing a `SKILL.md` file with YAML frontmatter (`name`, `description`, and optional metadata)
48:> plus a markdown body of process instructions. **57 skills** are authored here and available in every zed-kask install. **212 Jinja2 templates across 54
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

## Registry counts (re-verified 2026-10-30)

| Surface | Count | Notes |
|---------|-------|-------|
| `SKILL.md` directories (`.agents/skills/*/`, repo root) | **57** | filesystem count: `find .agents/skills -mindepth 2 -maxdepth 2 -name SKILL.md`. |
| Template namespaces (`kask/registry/templates/*/`) | **54** (**212** `.j2` templates) | Companion Jinja2 resources for `render_template`; counts come directly from the current tree. |

**The SKILL.md is the source of truth.** A skill is its `SKILL.md`. Template crates are
read-only resources the skill body may reference via `render_template`.

**Skills are evolving drafts (operator ruling 2026-09-26).** Every skill in `.agents/skills/`
is available to every install and every user. Skills and templates have no release state,
draft flag or developer-only tier: they change weekly or monthly through measured proposals,
not releases. Release vocabulary belongs to the Rust code, whose cadence is quarterly or
annual. The embedded seed payload equals the authored tree (pinned in
`crates/agent_skills/agent_skills.rs`).

---

## Skills (57)

### Capabilities you invoke (32)

The bench instruments — what a zed-kask user (the principal investigator) invokes for their research, coding, and creative work.

#### Research, markets and forecasting

| Skill | Purpose |
|-------|---------|
| `experimental-design` | Design discriminating hypothesis tests before scientific spend: normalize provenance-carrying hypotheses, compare materially different candidate designs, gate validity and information, bind an executor, and produce versioned DesignSpec and FindingsSpec artifacts |
| `hypothesis-testing` | Orchestrate one human-gated hypothesis-testing lifecycle: framing, falsifiability, experimental design, preregistration, executor-backed measurement, sealed findings, independent grounding audit, and human review |
| `eureka-cycle` | Run one human-chaired discovery cycle over the Open Questions board: intake and routing, recall-cited candidate generation, the PI funding gate, delegated lifecycles, sealed pre-registered experiments, resolution at the algedonic review, and a study report — a loop orchestrator and router that owns no lifecycle |
| `superforecasting` | Calibrated probability forecasting (Tetlock's Good Judgment Project) |
| `scenario-planning` | Complete scenario project: Schwartz framing, forces and divergent 2x2 narratives with a quality gate and early-warning indicators, Tetlock quantification and propagation, Brier-scored resolution, Chermack assessment |
| `eqm` | Explanation Quality Markers: score forecast rationales against the predictive 12 of the 60 EQMs via `market_score_rationale`, validate against realized outcomes (Brier), and improve a rationale in-session without changing its probability |

#### Media, writing and diagrams

| Skill | Purpose |
|-------|---------|
| `transcript-reel` | Source-linked research recordings: transcribe, correct, annotate, search lexically with citations, select highlights, render excerpts/reels, and export |
| `writing-style` | Compose or rewrite prose from curated style corpora and validate the result against measured style centroids |
| `diataxis-diagram` | Generate Mermaid diagrams from code using Diataxis methodology |

#### Decisions, research and evidence

| Skill | Purpose |
|-------|---------|
| `mcda` | Multi-Criteria Decision Analysis with compensation masking |
| `wardley-mapper` | Generic Wardley mapping: inventory components, classify evolution, map value chain, derive strategy |
| `hypothesis-framer` | Research question framing via FINER + PICO |
| `structured-extraction` | Extract structured data from unstructured text |
| `grounding-verify` | Verify factual claims in text against source data: extract claims, assign provenance, scan narrative, compute fact_score |
| `build-corpus-pipeline` | 10-stage corpus pipeline: convert → chunk → tag → embed → query → build_prompts → ingest_qa → assemble_dataset |

#### Working with the agent

| Skill | Purpose |
|-------|---------|
| `adhd-mode` | Session-scoped output mode shaping responses for a reader with ADHD: next-action-first, numbered steps, state restated across turns, capped lists, deterministic pre-send gate (render_template + lisp_eval), optional caveman compression variant (absorbed 2026-09-09) |
| `grill-me` | Socratic questioning to stress-test understanding |
| `improv` | Agent interaction grammar (Plussing, Yes And, Freestyling, Riffing) |
| `task-breakdown` | Convergent planning: vertical task slicing with acceptance criteria, checkpoints, and skill_match_query routing |
| `kanban-task-management` | Unified kanban task management across the full task lifecycle |
| `prompt-enhance` | General-purpose prompt enhancement: 7-type taxonomy routing with 3-tier effort knob |

#### Coding in your project

| Skill | Purpose |
|-------|---------|
| `code-review` | Convergent code review of a change against its stated spec: scope → multi-axis perspectives → adjudicate → report → optional implement |
| `diagnose` | Disciplined diagnosis loop: anchor → reproduce → hypothesise → instrument → fix → regression-test |
| `tdd` | Test-driven development: red → green, one vertical slice at a time, tests only at agreed seams; refactoring is not in the loop (code-review owns it) |
| `bug-hunt` | Bug hunting expeditions against target crates using Weinberg, Beizer, Bach, Hendrickson methodologies |
| `refactor-architecture` | End-to-end architecture refactoring: discover friction, rank candidates, walk design tree, audit duplication, plan strangler-fig migration, verify integrity |
| `idiomatic-rust` | Type-driven Rust design through Graydon Hoare's principles |

#### Disciplines and methods for your work

| Skill | Purpose |
|-------|---------|
| `deep-module` | Module design via Ousterhout's depth criteria (kask-operationalized deletion test, depth score, ≤7 surface cap) |
| `falsifiability` | Eliminative inference: Popper falsifiability gate, Chamberlin multiple hypotheses, Platt strong inference, Pearl counterfactuals |
| `gradient-hunter` | Find steep gradients between populated and unpopulated regions of a codebase/telemetry/test field |
| `lean-prover` | Machine-checked proof construction through Curry-Howard/de Bruijn/Carneiro lens. Sibling to falsifiability |
| `onto-anchor` | Resolve domain terms through the published-ontology fallback ladder before naming, classifying, or computing with them |

### Platform machinery (agent/operator-facing) (25)

These maintain the platform itself — the skill system, the Curator role, this fork's codebase, and this machine. They ship to every install like everything else (P5.1); this section labels their purpose, not their availability.

#### Fork-codebase development

| Skill | Purpose |
|-------|---------|
| `kask-seam-audit` | Convergent multi-skill audit of the zed-kask Kask-Zed seam (`DIVERGENCE.md` is the current numbered-seam authority); its measured-layout loop also runs standalone for GPUI cards and panels |
| `security-audit` | Target-scoped deep security audit of zed-kask code — a change, crate, MCP server, or named surface — against the OWASP LLM Top-10 (2025), MITRE ATLAS v5.1 and NIST SSDF SP 800-218A: deterministic evidence sweeps first, falsifier-backed findings citing file:line with verbatim evidence, defense-layer coverage verdicts, coverage honesty, and a hardening loop proposing ratchet entries for operator ratification (added 2026-10-24 with its four templates) |
| `mcp-tool-review` | Anchor a built-in MCP server's tools to published reference models and audit their functionality and quality — inventory reconciliation, fidelity-labeled checks, measured probes, bounded passes, and a findings artifact whose file:line citations are script-verified and routed to actuators |
| `provider-audit` | Two-directional provider contract audit over kask-adopted vendors: inbound (provider alignment with principles P1–P4 and values V1–V8) and outbound (our code's alignment with provider capabilities), over the profile layer at `kask/registry/providers/`; carries the provider lifecycle — adoption gate, drift trigger, scheduled cadence, announcement trigger, retirement pass |
| `upstream-rebase` | Manage upstream Zed rebases for zed-kask: per-D-seam-file strategy, mapped re-application, test-pin, DIVERGENCE.md update |
| `doc-update` | Realign the kask/docs tree with the code: condensation triage (<85 cap, working target 75), ground-compare-recompose per docs-set, file:line citation gates, corpus-tool decision point |
| `gpui-bench` | Design, write, review, run, and interpret production-shaped GPUI Criterion benchmarks (renderer/task benches, responsiveness, hang regressions, before/after evidence) |
| `verification-compression` | Compress a verification workflow without losing expectation coverage, falsifiers, failure visibility, provenance, or fault-detection signal; Lean-checked graph preservation |

#### Machine maintenance

| Skill | Purpose |
|-------|---------|
| `system-maintenance` | Weekly system maintenance for this machine over the `~/system-maintenance/` scripts: run the health check, interpret warnings, preview and classify cleanups with probe-before-delete, execute the disposable class only, surface manual-review items, verify with reconciled measurements, and report a ledger |

#### Governance and roles

| Skill | Purpose |
|-------|---------|
| `program-manager` | The agent's side of the Division of Responsibilities: recover the spec before building, design before coding, execute surgically, verify against a real definition of done |
| `product-manager` | The operator's side of the Division of Responsibilities: requirements as falsifiable outcome claims, spec provenance, acceptance criteria that can fail, ground-truth confirmation |
| `algedonic-review` | Human-in-the-loop review with the operator and Curator: alert triage, then the gemba walk — the only place skills are evaluated |

#### Curator methods and memory

| Skill | Purpose |
|-------|---------|
| `metacognition` | Improvement-Kata self-reflection: grasp, target, predict, experiment (including branching inquiry with delegation), measure the gap; predictions recorded for operator scoring |
| `pragmatic-cybernetics` | Feedback loops, variety engineering, system homeostasis |
| `pragmatic-semantics` | Classify statements by certainty, constraint force, provenance; route computation steps to deterministic or probabilistic machines (D/P labelling, P8.4) |
| `therapy` | Memory therapy session — scan curator memory for contradictions, fragmentation, and miscalibrated confidence; resolve, then reify lessons as skills/templates/rules |

#### Skill-system meta

| Skill | Purpose |
|-------|---------|
| `create-skill` | Author or translate a skill: ontology research, PDCA derivation, scaffold under the artifact contract, prescreen, validate |
| `skill-maintenance` | Validate and audit existing skills, including the template-logic audit of `.j2` goals and callsites and the prescreen's body-side D/P labelling presence and label-vs-tool checks (P8.4); compare designs and file proposals for the algedonic review |
| `skill-discovery` | Route tasks to installed skills (fit-scored recommendations), detect capability gaps, evaluate candidates before installation |
| `skill-bundler` | Merge peer-level skill outputs into one grounded unified report (per-skill summaries, cross-skill insights, explicit conflicts, one bounded correction) |
| `self-improvement` | Unified self-induced update operator: nested PDCA + outer Improvement Kata across two pathways — Foundation Model (θ) and Scaffolding (Σ), including the GEPA prompt-evolution sub-loop |

#### Agent behavioral disciplines

| Skill | Purpose |
|-------|---------|
| `coding-guidelines` | Enforce Karpathy's four coding principles: Think Before Coding, Simplicity First, Surgical Changes, Goal-Driven Execution |
| `essentialist` | Recursive eliminative interrogation (Exist → Surface → Contract) |
| `region-routing` | Route task steps across the syntax-semantic x deterministic-probabilistic 2x2 regions: named tools per step, a deterministic gate per generating step, role checkpoints at semantic and dependency boundaries |
| `kata-improvement` | 4-step Improvement Kata PDCA pattern, the five-question Coaching Kata (coach role), and beginner_mode drills |

