---
name: co-scientist
description: "Compose a human-gated scientific inquiry cycle over the experiment registry: frame a falsifiable hypothesis, declare a rubric-typed pre-registered experiment, seal the baseline, investigate variants through evaluation harnesses, record a disjoint final evaluation, and produce an evidence-audited study report."
---

# Co-scientist

Composes the full scientific inquiry cycle over the experimentation registry — the lab notebook. Each phase routes to the skill or tool that owns it; this skill owns the composition, never a phase's method. The cycle is human-gated by construction: selection happens at the algedonic review, and this skill proposes, never commits.

## Reference models

- The experiment protocol (`kask/docs/research/experimentation-reference-model.md` — the design authority): Declare → Vary → Test → Select → Retain, with pre-registered Brier-scored predictions, the headroom pre-check (§5 Q6), and human-chaired selection (§5 Q4).
- The Agentic Scientist (Lange, Grillotti, Yamada, Charakorn & Ha, arXiv:2610.18933, 2026) — the sealed-evaluation discipline this skill translates: sealed eval contracts, typed rubric bundles, disjoint final evaluation, and the typed conclusions update gate.
- Skillware (Fan & Lan, arXiv:2607.18970) — the SKILL.md + templates unit this skill and its `.j2` resources form.
- Kauffman's adjacent possible (*Investigations*, 2000) — every step of this composition is a one-step recombination of existing affordances; the skill introduces no new substrate.

## Initial and target condition

- **Initial condition:** a measurable question (any layer, including `scientific_hypothesis` — the general lab notebook, operator ruling D2 2026-10-15), the operator's go-ahead for a cycle, and the registry holding no open experiment for it.
- **Target condition:** a resolved experiment — declared with a typed rubric bundle, sealed on a measured baseline with headroom, investigated within budget, finally evaluated on a disjoint selection set, and selected or rejected at the algedonic review — plus an evidence-audited study report; the pre-registered claim Brier-scored by the linked kanban goal.

## Step types (D/P labelling)

| Step | Type | Oracle / critique |
|------|------|-------------------|
| Framing, rubric authoring, conclusions-gate application, report prose | P | the operator at the algedonic review; `grounding-verify` over the report's claims |
| Headroom, seal citation, budget ceiling, run-ref disjointness, claim Brier | D | the experimentation server's typed refusals; the goal loop |
| The conclusions gate form (`CS-1`) | D (`lisp_eval`) | the form's receipts |

## When to Use

- An end-to-end inquiry cycle on a measurable question: hypothesis → sealed experiment → evidence-audited study.
- A `scientific_hypothesis`-layer experiment on an external question (economic series, market resolutions, harness-measured artifacts).
- Composing an investigation across the research, scenarios, swarm, and experimentation servers.

## When NOT to Use

- Artifact-improvement experiments on zed-kask's own skills/prompts/adapters — `self-improvement` owns that machinery; route to it (this skill's declare/seal/measure steps still apply — the registry is shared).
- Forecasting without measurement — `superforecasting`; a forecast is not an experiment.
- Single-stage work (framing alone, elimination alone, audit alone) — invoke the owning skill directly.
- Bug diagnosis — `diagnose` + `falsifiability`.

## Instructions

1. **Frame.** Run `hypothesis-framer` (FINER/PICO) on the question; gate the H₁ through `falsifiability/falsifiability-admit` (render `falsifiability-admit` with the H₁ as `target`). Ground in the literature under a research run (`begin_research_run` → `web_search` / `resolve_paper` → `cite_sources` → `finish_research_run`). Derive the prediction confidence via `superforecasting` or `scenario_calibrate` — never self-assess it.
2. **Declare.** Render `co-scientist/co-scientist-frame` with the hypothesis, layer, genotype refs, and harness hints; take its `rubrics` and `declare_args`. Create the linked goal (`kanban_goal_create` — the goal loop Brier-scores the claim), then call `experiment_propose` with the rubric bundle, prediction `{claim, confidence}`, budget carrying `max_runs`, and `linked_goal_id`.
3. **Baseline & seal.** `variant_register` with `role=baseline`; measure it through the layer's harness (`swarm_eval_agent_local` with rubric-mapped evaluators, `training_evaluate` for adapters, or a measured observation via the FRED/DBnomics/World-Bank/market-resolution tools); `fitness_record` the report refs; then `experiment_seal`. A `no headroom` refusal (baseline saturated at 1.0) voids: `selection_record` with the first reject reason carrying the literal `no-headroom` marker, redesign the eval set (harder discriminators, never weaker evaluators), and re-declare fresh.
4. **Investigate.** `variant_register` challengers (and `role=ablation` variants with their parent) → execute through `swarm_fanout_local` / `swarm_execute_plan_local` with the rubrics' verification kinds as evaluators → `fitness_record` each variant's report refs citing `seal_id` → apply the conclusions gate (below) → iterate within `max_runs` (`BudgetExhausted` is terminal for the declaration).
5. **Final evaluation.** `final_evaluation_record` the leading variant on a selection set whose run refs the experiment has never recorded — the server enforces the disjointness; overlaps are typed refusals naming them.
6. **Select & report.** The algedonic review chairs selection; record its verdict via `selection_record` (`algedonic_reference` names the review record). Audit the study's claims with `grounding-verify`; write the study report with `cite_sources`; render the lineage (`lineage_read`) as a mermaid diagram via `diataxis-diagram`. File the durable lesson via `memory_insert` citing the goal's h_mem.

**The conclusions gate (per cycle, after each measurement batch).** Append the cycle's memory event via `memory_insert` (evidence-cited). Then update the distilled conclusion ONLY on: (a) contradiction — the cycle contradicts an existing sentence; (b) new category — a category no sentence covers; (c) flip — a Do/Don't recommendation reverses; (d) refinement — narrows, broadens, sharpens, or qualifies an existing sentence (name the dimension). Otherwise KEEP. Compute the gate deterministically with `lisp_eval` (`CS-1`):
- form: `(cond ((or (string= trigger "contradiction") (string= trigger "new_category") (string= trigger "flip") (string= trigger "refinement")) "update") (t "keep"))`
- env: `{ "trigger": <the classified trigger — "contradiction" | "new_category" | "flip" | "refinement" | "reconfirmation"> }`
Bound: 3 attempts per cycle on a rejected update; after the third, keep the prior conclusion and log `gate=skipped` in the memory event. Simple re-confirmation in the same regime is never sufficient.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `co-scientist-frame.j2` | FRAME — author the typed rubric bundle (≥1 metric rubric, ≥1 implementation or grounded_report rubric, stable snake_case ids, verification in the swarm evaluator vocabulary) and assemble the `experiment_propose` declaration arguments for the layer's harness. |
| `co-scientist-conclusions.j2` | CONCLUSIONS — classify the cycle against the prior distilled conclusion through the typed update gate (contradiction / new category / flip / refinement / reconfirmation) and emit the gated update or the keep. |

To render a template, call the `render_template` tool with the template ref (e.g. `co-scientist/co-scientist-frame`) and a context object with the required variables.

Template context variables (from each template's [inference] contract):
- `co-scientist-frame.j2`: `hypothesis`, `layer`, `genotype_refs`, `eval_set_hint`, `fitness_fn_hint`, `prediction_hint`
- `co-scientist-conclusions.j2`: `cycle_events`, `prior_conclusion`

## Regression case

Receipts executed live through `lisp_eval` and the experimentation server:

- `CS-1` conclusions gate: `trigger = "contradiction"` → `"update"`; each of `"new_category"`, `"flip"`, `"refinement"` → `"update"`; `trigger = "reconfirmation"` → `"keep"` (the only keep path — simple re-confirmation never updates).
- The headroom void path: a baseline measured at pass rate 1.0 → `experiment_seal` refuses with the no-headroom message → `selection_record(rejected, ["no-headroom: baseline saturated at 1.0 — redesign the eval set"])` → the claim is excluded from the calibration record (the `no-headroom` void marker).
- The route-does-not-reimplement check: every instruction step names an existing tool (`experiment_propose`, `variant_register`, `fitness_record`, `experiment_seal`, `final_evaluation_record`, `selection_record`, `lineage_read`); no step re-implements a delegated skill's method — `hypothesis-framer` owns FINER/PICO, `falsifiability` owns admission, `superforecasting`/`scenario_calibrate` own quantification, `grounding-verify` owns the audit.

## Constraints

- The seal is the gate: no challenger fitness pre-seal; every post-seal record cites the active `seal_id`; sealed fields are never edited — a changed contract is a fresh declaration with lineage.
- Recorded runs only — never simulated fitness; every run ref names a real harness or observation report.
- Nothing auto-adopts: selection is the algedonic review's chair (§5 Q4); this skill proposes, never commits.
- The study report is never labeled verified — `grounding-verify` emits findings with provenance strength; claims without sources are marked unanchored.
- The conclusions gate is typed: an update without a named trigger and dimension is a keep, not an update.
- The budget is the Layer-A set point: `BudgetExhausted` ends the declaration; re-testing a mechanism is a fresh declaration, never a re-dispatch past the ceiling.
