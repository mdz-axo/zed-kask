---
title: "Experimentation Reference Model — vary locally, select centrally, retain durably"
audience: [developers, architects, operators, agents]
last_updated: 2026-10-05
version: "1.0.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain, trust, lifecycle]
---

# Experimentation reference model

The reference model for zed-kask's sharded experimentation program: the design pattern, the shard map of current variation/fitness surfaces, the generalized experiment protocol, the guardrails and meta-metrics, the locked operator rulings, and the registry's connection map to the three-layer loop system. The execution records — the server build, the phased program, the outcome records, and the E-cycle history — remain in the repair plan (§P8); the built server surface is documented in [`reference/mcp-servers/experimentation.md`](../reference/mcp-servers/experimentation.md) and registered as `DIVERGENCE.md` D86/D87.

**Provenance.** Composed 2026-09-30 from the David Ha research run (research run `9c47de77a42b8ed8`, completed) — insight 3: evolution and population-based search as a first-class optimizer with small, structurally interpretable controllers; insight 4: the improve loop as itself an automatable, evolvable system — plus same-day tree probes: the GEPA sub-loop census, the experiment-template census, and the registered-MCP-surface census. Operator decisions locked 2026-09-30 (§5). Renamed from "Evolution" by operator ruling 2026-10-02: the mechanism is controlled experimentation — pre-registered hypotheses, fixed eval sets, deterministic evaluators, human-gated selection — not evolution. Promoted from the repair plan's §P8 to this file on 2026-10-05 per that plan's recorded promotion path (the docs condensation of 2026-10-05 freed the slot).

## 1. The design pattern: vary locally, select centrally, retain durably

The capability to evolve is already sharded across zed-kask's layers and stays sharded: each layer's genotype medium differs, so each layer owns its variation operators. What is shared is the ledger (comparable fitness and selection records), the selection surface (one human-chaired gate), and this named pattern.

| Element | In zed-kask |
| --- | --- |
| Genotype (small, legible, versioned) | SKILL.md **plus its `.j2` templates as one composite genotype** (§5, Q2); agent cards (system_prompt, model_params, evaluators); MCP tool schemas; regulation thresholds; LoRA adapters; goal/task definitions. Structure over weights: prefer evolving text/config to fine-tuning when both could serve — cheaper, reviewable, revertible. |
| Phenotype | A running session/agent/server expressing a genotype configuration. Fitness is measured on phenotypes; selection edits genotypes. |
| Variation (sharded) | Mutation (GEPA prompt evolution; skill edits; `evolve_mcp_tool_schema` directives), recombination (adapter merging under merge contracts; skill bundling; roster composition), continuous calibration (thresholds — the scalar ES analog). |
| Fitness (grounded only) | Deterministic evaluators, verifier gates, Brier scores on resolved outcomes, calibration readings, algedonic verdicts. Never LLM-only (the anti-gaming rule). Recorded runs only — never simulated (the GEPA house rule). |
| Selection (central, human-chaired) | Automated pre-selection only within declared noise bands (GEPA's 10% cost band is the precedent); final selection at the algedonic review (§5, Q4) or a recorded grant. Nothing auto-adopts. |
| Retention & lineage | Git is the germline (proposal → delegated implementation → verification). The experiment registry is the fossil record, including rejected variants with reasons. Curator memory holds acquired traits, feeding variation through therapy's reification (the Baldwin channel). |

Two zed-kask-specific syntheses:

1. **The pre-registered, Brier-scored prediction is the tiny controller.** The experiment machinery (swarm rollouts, eval harnesses, training jobs) is the big shared commodity part; the hypothesis plus prediction is the small, legible, evolvable part; the Brier score is the selection signal on the experimenter, not just the artifact. This is the advantage over the source setting: Ha's fitness functions are external benchmarks; this system scores its own hypotheses.
2. **D/P labelling is the legibility constraint that makes variants selectable.** The variant carries its own deterministic gates, declared evaluators, and lineage — it explains itself, so selection is auditable.

## 2. Current state — the shard map (probe evidence, 2026-09-30)

| Layer | Genotype | Variation today | Fitness today | Gap |
| --- | --- | --- | --- | --- |
| Prompt/template | system prompts, `.j2` | GEPA sub-loop (mutation+crossover, Pareto frontier, pinned D forms) | `swarm_eval_agent_local` on disjoint feedback/selection sets; 10% cost noise band | "Prompts only (v1)"; no cross-run lineage |
| Weights (FM) | LoRA adapters | retrain mode; verdict-bridged rollouts (SFT/DPO) | held-out eval; A/B on job completion | runs not registered or comparable |
| Skill | SKILL.md + templates | skill-maintenance edits; therapy reification | fixed-task comparisons; pin tests | ad hoc A/Bs, no records |
| Agent/swarm | agent cards | reconfigure; roster composition | `swarm_eval_agent_local`; task boards | no variant populations or retention |
| Tool schema | MCP schemas | `evolve_mcp_tool_schema` directive | skill-use reports | fitness loop thin (issues, not outcomes) |
| Regulation scalars | thresholds, budgets | `calibrate_threshold` directives | variety deficit, Brier, calibration readings | calibration by judgment, not recorded fitness |
| Memory | h_mems, rules | insert/update; therapy | recall/decay, contradictions | acquired vs heritable not distinguished |

Evidence notes:

- GEPA sub-loop: `.agents/skills/self-improvement/SKILL.md` ("Prompt evolution (GEPA)" section; templates `gpa-sample-trajectories.j2`, `gpa-reflect.j2`, `gpa-propose-mutations.j2`, `gpa-test-variants.j2`, `gpa-frontier-update.j2`); dominance and convergence forms executed live through `lisp_eval` (receipts in the skill-audit records). Scoped "Prompts only (v1)"; disjoint feedback/selection sets; recorded runs only.
- Experiment-concept shards (each skill its own notion, no shared record): `kask/registry/templates/eqm/eqm-imp-experiment.j2`, `kask/registry/templates/kata-improvement/coaching-q4-experiment.j2`, `kask/registry/templates/kata-improvement/improvement-step4-experiment.j2`, `kask/registry/templates/metacognition/meta-experiment.j2`, `kask/registry/templates/verification-compression/experiment.j2`.
- No experimentation/experiment MCP server registered (list_mcp_tools census, 2026-09-30). *(Superseded the same day: `hkask-mcp-experimentation` was built 2026-09-30 as the 13th managed server — DIVERGENCE.md D86; the census row records the pre-build probe.)*
- GEPA run logs persist per-run at `~/Documents/zk-data/skills/self-improvement/gepa/{date}-{run}/` — directories, not a queryable registry. *(Superseded 2026-09-30: the registry is the record path; historical directories on disk are data, untouched.)*

## 3. The generalized experiment protocol

Generalizes GEPA (prompts-only v1) to the artifact classes. **The skill genotype is composite** (§5, Q2): SKILL.md process text and its `.j2` template resources evolve together; a skill experiment's variants are (SKILL.md, templates) pairs, and pin suites re-run in the same change.

| Artifact class | Eval-set requirement | Fitness | Noise band / gates |
| --- | --- | --- | --- |
| Skill (composite: SKILL.md + templates) | Fixed task set plus the skill's pin tests | Evaluator pass rates on the fixed set; pin suites green | Pin-suite gate mandatory; D/P labels preserved |
| Agent card | Fixed task set with declared evaluators | `swarm_eval_agent_local` pass_rate, total_tokens | 10% cost band (GEPA precedent) |
| Standalone prompt/template | Runnable eval set, disjoint feedback/selection | Eval-set scores | GEPA dominance/convergence forms |
| Tool schema | Skill-use reports over a window | Issue counts, resolution outcomes | Curator directive ledger |
| Regulation scalar | Calibration/Brier records over a window | Calibration delta | Curator thresholds |
| LoRA adapter | Held-out task set | `training_evaluate` scores | lora-training gates G-M1..G-Q5 |

Protocol steps, every experiment:

1. **Declare** — hypothesis, genotype config, eval set, fitness function, noise band, pre-registered prediction with confidence, energy budget → `experiment_propose` (creates the linked kanban goal; the prediction is Brier-scored at verdict).
2. **Vary** — mutation/crossover per the layer's operator; each variant registered with its parent (lineage).
3. **Test** — fitness from recorded runs only; feedback and selection sets disjoint wherever the operator is a model. **Headroom pre-check first (§5, Q6):** the baseline variant runs alone before any challenger spend — a baseline at 1.0 (100% of the test set) voids the experiment at design time as `rejected` with a no-headroom reason, and the eval set is redesigned (harder discriminators, never weaker evaluators) and re-declared; the pre-check is budgeted auto-run (§5, Q7).
4. **Select** — deterministic pre-selection within noise bands; final selection at the algedonic review (§5, Q4). Deterministic-fitness experiments auto-run within the declared budget (§5, Q1); human-judged-fitness experiments route through the operator before running.
5. **Retain** — selected variants land in git through the proposal → delegation → verification path (proposes, never commits); rejected variants persist as fossils with reasons; lineage recorded.

## 4. Guardrails, meta-metrics, research anchors

**Guardrails** (each tied to a project rule): grounded fitness only — the anti-gaming trio (real sources, load-bearing properties preserved, external ground truth); algedonic selection — nothing auto-adopts; proposes-never-commits; D-seam entries plus tests in the same change; SKILL.md edits re-run pin suites; scripts are bash; new crates declare `[lib] path`; no `mod.rs`; no `unwrap()`; no backward compatibility (§5, Q5) — replace superseded paths directly and delete them in the same change: no deprecated APIs, legacy adapters, compatibility flags, dual writes, or parallel implementations; every phase names its replaces; energy budgets price the loop.

**Meta-metrics:** experiment-prediction Brier (primary — the system learning to predict its own improvements; computed from the registry, reported at algedonic review); retention rate (selected variants still active after N weeks); throughput (proposals → registered → completed → selected/rejected per cycle); variety (genotype distribution in active use — the curator's variety-deficit metric extended to experimentation).

**Research anchors** (all retrieved in research run `9c47de77a42b8ed8`): population search over hill-climbing — EvoJAX (Tang, Tian & Ha, 2022); Recurrent World Models (Ha & Schmidhuber, 2018). Small legible controllers — Weight Agnostic Neural Networks (Gaier & Ha, 2019); World Models (Ha & Schmidhuber, 2018). Structural interpretability — Neuroevolution of Self-Interpretable Agents (Tang, Nguyen & Ha, 2020). The loop as the target — The AI Scientist (Lu et al., 2024); The AI Scientist-v2 (Yamada et al., 2025); Towards end-to-end automation of AI research (Yamada et al., Nature, 2026). Recombination — Evolutionary optimization of model merging recipes (Akiba et al., 2024). Adversarial and open-ended search — Digital Red Queen (Kumar et al., 2026); Automating the Search for Artificial Life with Foundation Models (Kumar et al., 2024).

## 5. Decision log (operator rulings, 2026-09-30)

| Decision | Ruling | Consequence |
| --- | --- | --- |
| Q0 — docs slot | Not answered in the locking reply; proceeded with the host-as-section recommendation (the repair plan §P8, per the P7f precedent) to keep the locked decisions moving. Promotion path recorded; an operator override (cap raise or condensation-first) splits the section into a research file at any time. | The reference model was durable in the plan; the promotion to this file executed 2026-10-05. |
| Q1 — loop autonomy | **b — budgeted auto-run.** Deterministic-fitness experiments auto-run within a declared energy budget; the operator chairs selection at the algedonic review with itemized spends; human-judged-fitness experiments route through the operator before running. | The loop runs between reviews; the operator keeps the selection chair and the budget lever. |
| Q2 — first scope | **a+b — skills and templates together, plus agent cards.** The skill genotype is composite: SKILL.md and its `.j2` templates evolve as one unit ("you need to evolve the skill.md with the jinja2 templates" — operator, 2026-09-30). | Skill experiments mutate process text and templates in one variant; pin suites re-run in the same change. |
| Q3 — server shape | **B — build `hkask-mcp-experimentation` now.** Demand and requirement are proven by the existing GEPA machinery and the active work thread (operator, 2026-09-30); the server lands together with the protocol. | Phase 1 built the 13th managed server; the program is re-phased around its schema. |
| Q4 — human gate | **A — algedonic review as the single selection surface.** | One board, one fossil record; selection concentrates in the existing gemba walk. |
| Q5 — compatibility posture | **No backward compatibility — pre-release** (operator, 2026-09-30; the plan-level §1 ruling applied to this program). Replace superseded paths directly, update all callers together, delete old paths in the same change; no deprecated APIs, legacy adapters, compatibility flags, dual writes, or parallel implementations; no legacy-import surface in the server. | Phase 1 is replacement-first: the GEPA retention mechanism and per-skill record formats are deleted as the registry lands, not kept as fallbacks. |
| Q6 — eval-set headroom | **A — baseline headroom pre-check** (operator, 2026-09-30). Before any challenger spend, the baseline variant runs alone; a baseline pass rate of 1.0 (100% of the test set) voids the experiment at design time as `rejected` with a no-headroom reason, and the eval set is redesigned — harder discriminators (conservation checks, exact labels), never weaker evaluators — before re-declaring. | Saturation discovery costs one variant's spend, not two (the trap was observed twice in the first generation); wired into §3 step 3 and the agenda generator. |
| Q7 — pre-check autonomy | **A — the pre-check is budgeted auto-run** (operator, 2026-09-30). The baseline-only measurement is deterministic-fitness work inside the declared budget under the Q1=b ruling; the operator chairs selection, not baseline measurement. | The agenda loop self-screens its queue; the operator reviews only screened experiments. |

## 6. Connecting the registry to the three-layer loop system (CNS reference model)

The registry is already an expectation-carriage machine in CNS terms ([`research/cybernetic-nervous-system-reference-model.md`](cybernetic-nervous-system-reference-model.md)): every experiment stores its expectation before measurement (the pre-registered prediction — INV2), records the observation (fitness from recorded runs), and propagates the delta (claim held/refuted; Brier — the prediction-error signal). Its reporting is surprise-gated by construction (INV3): the headroom pre-check voids eval sets that cannot produce prediction error, and the agenda generator turns regulation deviations — the system's own prediction errors — into candidates. Selection is model revision at the top (INV5): a selected variant changes an artifact, which changes what Layers A and B expect; a rejected variant persists as a fossil so the model remembers.

**Connection map** (registry element ↔ CNS layer ↔ pathway ↔ status):

| Registry element | Layer (loop) | Pathway | Status |
| --- | --- | --- | --- |
| Agenda signal reads (`curator_semantic_search`, `curator_algedonic_log`, `curator_status`, `population_query`) | B, sensing A and C (L2, L16) | afferent into B | wired (Phase 3) |
| `experiment_propose` / `variant_register` / `fitness_record` / `selection_record` from sessions | B (L1 tool dispatch → L4 server) | efferent (in-thread actuation) | wired (Phases 1–3) |
| Headroom pre-check (§5, Q6) | B | INV2/INV3 gate | wired (first live operation in the E3 cycle) |
| Algedonic review card → operator verdict → `selection_record` | C (L16) | efferent (authority down) | wired (Q4) |
| Goal-loop Brier scoring of experiment predictions; calibrated confidences descend to the next agenda | C (L9 + L10) | afferent outcome up, efferent calibration down | wired |
| Regulation loop sensing of the experimentation server's experiment health | A (L2) | afferent | wired (2026-09-30, D87 — `ExperimentationStuckExperiments` sensor + bridge source over `ExperimentationStore::health_snapshot()`) |
| The declared budget as a Layer-A set point with local actuation | A (L2) | efferent | wired (D-1: `budget.max_runs` required at declaration, server-enforced at `fitness_record` with typed `BudgetExhausted`) |
| Discarded-signal persistence and recurring-discard escalation | C (L16) | INV4 | wired (skill text step 5: every discard persists in the cycle card; a two-cycle recurring discard escalates as its own algedonic card) |
| Selection → adoption → artifact-version join | C → A/B | INV5 efferent | wired as the documented path (card commit hash → `algedonic_reference` → `lineage_read`) |
| Curator energy budget regulating experimentation spend | A + C (L2) | efferent | wired (D-2: `max_runs` set point + existing call-cap meter + health sensor; curator status surfaces it) |

The wiring records — the six steps' landed state, the follow-up fixes (selection-record race, measurement-void marker), and the E3–E7 cycle outcomes — live in the repair plan §P8.9; this section carries the durable map.

## Where the execution records live

- [`plans/hkask-core-mcp-repair-improvement-plan.md`](../plans/hkask-core-mcp-repair-improvement-plan.md) §P8 — the server build (P8.4), the phased program (P8.5), the Phase-2 outcome record (P8.8), and the wiring/cycle records (P8.9).
- [`reference/mcp-servers/experimentation.md`](../reference/mcp-servers/experimentation.md) — the built server's tool surface (six tools over one SQLCipher registry).
- `DIVERGENCE.md` D86 (the server) and D87 (the experimentation health source).
