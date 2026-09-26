---
name: lora-training
description: "LoRA/QLoRA training configuration and contract enforcement for hKask. Produces an advisory PEFT recommendation through fixed-order evidence gates; the operator selects the method and the runtime enforces hard contracts."
---

# LoRA Training

Recommend a composable PEFT configuration from declared evidence, audit the
operator-selected configuration without replacing it, report normalized
findings losslessly, and compute phase-aware training-readiness convergence.
This skill does not train, load, initialize, merge, or evaluate models.

## Initial and target condition

- **Initial condition:** declared operator requirements, host, base model/config inputs and dataset-format hint; after operator selection, the concrete training parameters, optional dataset path, and any observed runtime/post-training evidence. An absent dataset validation is not a zero-risk dataset.
- **Target condition:** a sourced advisory recommendation that preserves operator choices, followed (only for a selected concrete configuration) by a phase-aware audit with every applicable gate accounted for. Readiness is `Pass` only on complete observed coverage with no blocking/conditional findings; missing static evidence never becomes `Pass`, and future runtime/post-training checks remain unmeasured until observed. A first complete pass ends the local loop.

## When to Use

- Before training, to obtain an evidence-grounded PEFT recommendation while
  preserving explicit operator requirements.
- After the operator accepts, overrides, or rejects that recommendation, to
  audit the selected concrete configuration and declared harness.
- When runtime or post-training measurements are supplied, to assess established
  contracts without fabricating execution results.
- To report training findings, readiness, and contract gaps.
- To compute convergence for the current lifecycle phase and expose preflight,
  runtime-contract, and post-training posture separately.
- To recommend a declarative training harness (Axolotl or Ludwig) and method
  based on task requirements, data shape, and — when supplied — the full
  capability space (2 harnesses × 5 methods × 3 hosts × cost models).
- When prior training history, prior PDCA iteration output, prior outcome
  evidence, or prior operator feedback is available, to refine the
  recommendation via Good Regulator compliance and self-improvement loop
  closure.

## Authority and Boundary

- **Skill:** recommends.
- **Authenticated operator:** accepts, overrides, or rejects; the selected method
  and explicit requirements remain authoritative.
- **Runtime:** enforces established hard contracts against the accepted concrete
  configuration.
- **Recommendation is not readiness:** selection leaves readiness undetermined
  until audit evidence establishes it.
- Read only declared workspace paths. Do not download models, call remote
  services without explicit consent, or execute initialization, forward,
  backward, merge, training, or evaluation.
- Require `host` for every action.

## When NOT to Use

- Prompt optimization — use `prompt-enhance`; this skill governs weight-space training configs.
- Executing the training run — the training server and `adapter-lifecycle` own execution; this skill recommends and audits configs.
- Security findings — out of scope by its own template note; this skill owns training-config recommendation and contract evidence.

## Instructions

Process order: recommend with `select-method` from declared evidence → operator selects a concrete configuration → optionally validate a declared dataset with `training_validate_config` → audit selected configuration → report. The sections below are lookup steps, not evidence that a runtime tool ran.

### `lora-training/preflight-dataset`

1. After the operator selects concrete `params`, call `training_validate_config` with those params and the declared `dataset_path` (and `base_model` when known); unwrap the tool envelope and use its actual `dataset_format` and `findings`. A render of `preflight-dataset.j2` does not read the file or run this validation. A tool error leaves G-D0 unassessed.
2. Report the runtime's four-state `dataset_format.verdict`: `ready`, `needs_mapping`, `incompatible`, or `undetermined` when detection/evidence fails. Do not convert `undetermined` to `ready`.
3. When `needs_mapping`, report the runtime's `mapping_code` as an unexecuted external-harness suggestion; do not add a Python script or dependency to this Rust repository. `ready` requires an observed ready verdict from `training_validate_config`, not an assumption that normalization ran.
4. This phase is skipped when `dataset_path` is absent, with a named evidence gap; pass `dataset_validation: {}` to audit-config and never imply an observed format. The skill does not execute training or modify dataset files.

### `lora-training/select-method`

1. Read the declared training inputs and preserve explicit operator requirements.
   Consume `prior_iteration` (this session's previous PDCA turn),
   `prior_training_history` (Good Regulator), and `provider_capabilities` (deep capability reasoning) when supplied.
2. Refine one composable *advisory* recommendation through eight fixed-order gates (P: evidence-based judgments, not a deterministic computation): adapter
   purpose (G0), dataset analysis (G-D0), inference constraint (G1), memory
   evidence (G2), task distance (G3), quality/cost (G4), knowledge
   preservation (G5), and harness capability (G6). Training approach
   selection (G0-G5) precedes harness selection (G6). Gates refine
   compatible fields; they do not overwrite the whole recommendation or
   silently replace earlier constraints.
3. Emit only derivable values for `adapter_purpose`, `base_mode`, `adapter_form`,
   `scaling`, `initializer`, `preservation`, `rank_range`,
   `target_module_strategy`, `harness`, and `training_method`; otherwise emit
   `undetermined`, required evidence, alternatives, constraints, or conflicts.
4. Compute `(* model_size_b 2)` via `lisp_eval` when `model_size_b` is supplied; treat the result only as an approximate bf16 base-weight floor, not a fit prediction.
   Memory pressure may favor QLoRA, but these two scalar inputs do not establish
   that a configuration fits or will OOM. When `prior_training_history.prior_oom_patterns`
   is supplied, refine G2 using operator-specific OOM evidence (Good Regulator)
   without fabricating OOM certainty.
5. Preserve operator-requested initializers uniformly. For EVA, report
   `initialize_lora_eva_weights(model, dataloader)` as required evidence; do not
   hardcode a recommendation-phase refusal.
6. G0 (adapter purpose) establishes what kind of adapter is being produced
   (instruction, reasoning, vision, preference, reward_model). This
   determines baseline rank ranges, target module strategies, and the
   learning-forgetting tradeoff posture. G0 runs first and constrains all
   subsequent gates. When `prior_training_history.prior_rank_choices` is
   supplied, refine G3 within the G0 baseline using operator-specific rank
   evidence (Good Regulator) — prior choices refine, they do not replace.
   At recommendation time, G-D0 uses only declared `dataset_format_hint` and operator requirements. `training_validate_config` needs a concrete `params` object and runs after selection: with `dataset_path`, it returns `dataset_format` and G-D0/G-D1 findings, not sample-length, role, vision or preference-balance statistics. Do not refine G3 rank, G6 harness, or adapter purpose from a profile this tool does not return. When format is unknown, report the required evidence instead of inventing dataset characteristics.
7. G6 (harness capability) selects a harness based on the training approach
   determined by G0-G5. The harness must be able to efficiently process the
   declared dataset and produce the adapter type implied by G0. When
   `provider_capabilities` is supplied, G6 reasons over the full capability
   space (2 harnesses × 5 methods × 3 hosts × cost models): available_hosts,
   host_gpu_types, host_cost_models, and inference_provider_capabilities.
   When absent, G6 falls back to harness-method compatibility only. If the
   operator declares `harness_preference` or `trainer_preference` inputs,
   preserve them as `operator_requested` and validate compatibility. If both
   are absent, select based on adapter_purpose and dataset_format_hint. The
   two retained harnesses have distinct capability profiles:
   - **Axolotl** (YAML, SFT): mature, single-file configuration and the runtime
     default for instruction adapters.
   - **Ludwig** (YAML, SFT + DPO + KTO + ORPO + GRPO): declarative like Axolotl,
     but covers preference optimization, GRPO, and advanced PEFT initializers
     (PiSSA, EVA, CorDA, LoftQ).
   Axolotl remains the runtime default when harness is undetermined and
   adapter_purpose is instruction — no silent migration. For non-instruction
   purposes, axolotl is not a valid default.
8. The selected configuration starts a bounded PDCA: audit the applicable current-phase gates, check their coverage and states, then route the readiness verdict, `blockers`, and `gate_results_summary` into `prior_iteration` only when a concrete, operator-authorized refinement is possible. First call `lisp_eval` on gate IDs *before* computing readiness:
   - form: `(begin (define all-present (lambda (xs ys) (if (is_null xs) t (and (member (car xs) ys) (all-present (cdr xs) ys))))) (and (> (length expected_gate_ids) 0) (= (length expected_gate_ids) (length observed_gate_ids)) (all-present expected_gate_ids observed_gate_ids) (all-present observed_gate_ids expected_gate_ids)))`
   - env: `expected_gate_ids` from the gate catalog applicable to the current selected method/phase; `observed_gate_ids` from actual gate results (not just the training server's subset). A missing, duplicated, or empty gate list is `Not evaluated`; do not pass a partial list to the readiness rule as if it were complete. Coverage is structural, not proof that each finding is correct.
   Then compute the readiness verdict from the complete current-phase states with `lisp_eval` using `report.j2`'s precedence (Refuse > Fail > Conditional > Deferred > Not evaluated > Pass):
   - form: `(begin (define has (lambda (s l) (if (is_null l) nil (or (string= (car l) s) (has s (cdr l)))))) (cond ((is_null states) "Not evaluated") ((has "refuse" states) "Refuse") ((has "fail" states) "Fail") ((has "warn" states) "Conditional") ((or (has "deferred" states) (has "planned" states)) "Deferred") ((has "not_evaluated" states) "Not evaluated") (t "Pass")))`
   - env: `{ "states": [<state of every gate applicable in the current phase>] }`
   A complete current-phase `Pass` stops the local loop. Future runtime and post-training requirements remain separately `deferred` or `planned`, not evidence of a preflight pass or reason to rerun a recommendation without new inputs. No coverage or unchanged evidence means `Not evaluated`/blocked, not another turn. Bound: at most 3 operator-authorized refinement turns; report remaining blockers rather than repeating. Readiness is state-based, not a weighted convergence metric.
9. Return separate `recommendation`, `readiness`, `justification`, and
   `authority` objects.

### `lora-training/audit-config`

1. Audit the operator-selected method unchanged. Keep advisory recommendations
   separate from readiness.
2. Read only declared config and harness artifacts. Quote exact paths, lines,
   parameters, values, and snippets; unavailable evidence members remain null.
3. Classify each gate into exactly one phase:
   `static_config | harness | runtime | post_training`.
4. Use exactly one state per gate:
   `pass | warn | fail | refuse | deferred | planned | not_evaluated | not_applicable`.
   Missing evidence is not failure. Runtime or post-training requirements without
   measurements are `deferred`, or `planned` when a concrete supplied plan exists.
5. Use exactly one evidence kind:
   `config_value | code_presence | code_absence | runtime_measurement | operator_assertion | not_available`.
   `code_absence` requires a search of the complete declared harness scope.
6. Apply all 19 gates phase-appropriately: G-M1..G-M5, G-Q1..G-Q6,
   G-D1..G-D3, G-F1..G-F2, G-H1, G-R1 (runtime alert), and G-P1 (persistence
   preflight). Runtime and post-training passes require supplied measurements;
   this template never executes those checks. Pass `dataset_validation` as the actual unwrapped `training_validate_config` response (`dataset_format`, G-D0/G-D1 `findings`, `gates_evaluated`) when it ran; otherwise pass `{}` and mark those checks unassessed. A warning-free response does not establish an unreported sample count or dataset quality. Consume `runtime_metrics`
   for G-R1 runtime alert assessment (loss spikes, NaN gradients, vanishing
   loss) when supplied. G-P1 verifies HuggingFace artifact persistence is
   configured before submit on ephemeral cloud hosts.
7. Inspect initializer-specific preprocessing and persistence according to the
   selected initializer's documented contract. Do not introduce an EVA-specific
   or framework-version-specific refusal rule.
8. Enforce no-fiction mechanically: findings with `evidence_kind` of
   `config_value`, `code_presence`, or `code_absence` MUST have non-null
   `evidence.config_path` AND non-null `evidence.line`. Findings that fail this
   check are rejected at the audit gate and counted in `rejected_findings` with
   reason `"missing_citation"`. Findings with `evidence_kind` of
   `not_available`, `operator_assertion`, or `runtime_measurement` are exempt.
9. Return a `refuse_escalation` entry for every `refuse` finding, carrying
   `finding_id`, `gate_id`, `claim`, `requirement`, `evidence`, `selected_method`,
   `host`, and `severity: critical`. The invoking agent surfaces it immediately
   to the operator; a template response alone does not create a durable alert.
   Downstream phases still preserve the finding.
10. Emit every result using the normalized Finding schema below and compute
    readiness separately.

### Normalized Finding Schema

Every finding has exactly these fields:

- `finding_id`
- `gate_id`
- `phase`: `static_config | harness | runtime | post_training`
- `state`: `pass | warn | fail | refuse | deferred | planned | not_evaluated | not_applicable`
- `severity`: `critical | high | medium | low | informational | none`
- `selected_method`
- `readiness_impact`: `blocking | conditional | non_blocking | none | unknown`
- `claim`
- `requirement`
- `evidence_kind`: `config_value | code_presence | code_absence | runtime_measurement | operator_assertion | not_available`
- `evidence`: `{config_path, line, parameter, value, snippet}`
- `provenance`: `direct | inference | assessment | operator`
- `epistemic_mode`: `declarative | probabilistic | subjunctive`
- `citation`
- `recommendation`
- `host`

Do not create alternate finding shapes. A recommendation never overwrites
`selected_method`, and unavailable evidence never becomes an observed violation.

### `lora-training/report`

1. Render `lora-training/report` with the unchanged operator `selected_method`, `host`, normalized `findings`, `gate_results`, and the `computed_readiness` returned by Step 8's coverage and precedence checks. Reject missing inputs rather than synthesizing a method or verdict. Consume normalized findings without adding, removing, renaming, repairing, or reclassifying fields.
2. Present complete findings unchanged; grouped views may organize them by phase,
   state, or severity only.
3. Report counts for all eight states and four phases. Keep selected method,
   advisory method recommendations, and readiness separate.
4. Record `deferred`, `planned`, and `not_evaluated` requirements as contract
   gaps with the next evidence needed; exclude `not_applicable`. Do not mutate
   findings to create gaps.
5. Preserve `computed_readiness` unchanged. The caller's `lisp_eval` checks own gate-ID coverage and precedence (`Refuse > Fail > Conditional > Deferred > Not evaluated > Pass`); a different method recommendation cannot change the result. If coverage was not checked, report `Not evaluated`, never an inferred pass.
6. Preserve claim-appropriate citations and report exact phase, state,
   severity, and evidence-kind counts.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `preflight-dataset.j2` | Deterministically present the observed `training_validate_config.dataset_format` four-state verdict and findings after operator-selected params; missing format is `undetermined`, never ready. Does not execute mapping code or training. |
| `select-method.j2` | Apply a deterministic 8-gate refinement without overwriting earlier constraints or operator requirements. G6 reasons over the retained capability space (2 harnesses × 5 methods × 3 hosts × cost models) when provider_capabilities is supplied. G2 and G3 refine using prior_training_history when supplied (Good Regulator compliance). Consumes prior_iteration when present (the previous in-session PDCA turn). |
| `audit-config.j2` | Audit the selected configuration with the applicable subset of 19 declared gates, preserving citations and refuse escalations. Consumes actual `dataset_validation` findings when available and `runtime_metrics` for G-R1 when supplied; unmeasured gates remain unassessed. G-P1 checks declared persistence setup before submit. |
| `report.j2` | Synthesize audit findings with concrete config evidence, source citations (arXiv paper sections + PEFT v0.19.0 doc sections), severity (critical/high/medium/low), gate ID, and remediation. Preserve the normalized Finding schema, identify contract gaps, and separate recommendation from phase-aware readiness. Produce verdicts from evidence-backed states without reclassifying findings. |

To render a template, call the `render_template` tool with the template ref (e.g., `lora-training/preflight-dataset`) and a context object with the required variables.

## Constraints

- This SKILL.md body is the authoritative methodology. Jinja2 templates in the registry are structured reference versions of the same content.
- All four templates are public. No hidden training controls or parameters.
- Preserve operator sovereignty and authenticated `host` identity.
- Emit only values, findings, states, citations, and measurements supported by
  declared evidence. Do not invent defaults, snippets, line numbers, benchmark
  results, or training outcomes.
- No-fiction enforcement is mechanical: findings with
  `config_value`/`code_presence`/`code_absence` evidence_kind and null
  `config_path`/`line` are rejected at the audit gate, not merely discouraged.
- Algedonic escalation: `refuse` findings emit `refuse_escalation`
  in-addition to normal flow so safety-boundary violations reach the operator
  before the full pipeline completes.
- Convergence honesty: `not_evaluated` (a coverage gap) is distinct from
  `deferred`/`planned` (a known unmet requirement); the readiness precedence
  keeps them apart and neither ever reads as `Pass`.
- Runtime and post-training gates are requirements or assessments of supplied
  measurements; the skill does not execute them.
- Regression proposals are human-reviewed, `status: pending`, and
  `surface: training`.
- Evaluation is separated from execution (Goodhart): the runtime records
  `reg.skill.lora-training.outcome` when the skill activates, and the operator
  rates recommendations only through `record_skill_feedback` in the Curator's
  algedonic review. Review findings change this SKILL.md through a gemba-walk
  proposal; the skill never reads verdicts back to calibrate itself.
- Security review of training infrastructure is a separate concern owned by
  security-audit practice; `tdd` owns training-loop code correctness;
  this skill owns training-configuration recommendation and contract evidence.

## Source References

- LoRA: [arXiv:2106.09685](https://arxiv.org/abs/2106.09685) — initialization,
  merge, scaling, rank, and trainable-parameter contracts.
- QLoRA: [arXiv:2305.14314](https://arxiv.org/abs/2305.14314) — NF4,
  quantized training, paged optimizers, data quality, and evaluation.
- rsLoRA: [arXiv:2312.03732](https://arxiv.org/abs/2312.03732) — `α/√r` scaling.
- DoRA: [arXiv:2402.09353](https://arxiv.org/abs/2402.09353).
- PiSSA: [arXiv:2404.02948](https://arxiv.org/abs/2404.02948).
- LoRA-GA: [arXiv:2407.05000](https://arxiv.org/abs/2407.05000).
- EVA: [arXiv:2410.07170](https://arxiv.org/abs/2410.07170).
- Razin et al.: [arXiv:2410.21228](https://arxiv.org/abs/2410.21228) — intruder
  dimensions and structured forgetting.
- Biderman et al.: [arXiv:2405.09673](https://arxiv.org/abs/2405.09673) — LoRA
  Learns Less and Forgets Less. LoRA underperforms full FT on code/math at low
  rank; high rank (r=256) can match full FT on IFT but not CPT. LoRA forgets
  less — a feature for knowledge preservation. Rank is the learning-forgetting
  knob.
- Thinking Machines Lab: [LoRA Without Regret](https://thinkingmachines.ai/blog/lora)
  — For SFT on small-to-medium instruction/reasoning datasets, LoRA performs
  the same as full FT. For datasets exceeding LoRA capacity, LoRA underperforms.
- AutoPEFT: [arXiv:2301.12132](https://arxiv.org/abs/2301.12132) — rejected
  per-job multi-objective search alternative.
- DPO: [arXiv:2305.18290](https://arxiv.org/abs/2305.18290) — Direct Preference
  Optimization.
- KTO: [arXiv:2402.01306](https://arxiv.org/abs/2402.01306) — Kahneman-Tversky
  Optimization.
- ORPO: [arXiv:2403.07691](https://arxiv.org/abs/2403.07691) — Odds Ratio
  Preference Optimization.
- GRPO: [arXiv:2402.03300](https://arxiv.org/abs/2402.03300) — Group Relative
  Policy Optimization (reward-model-free RLHF).
- PEFT v0.19.0:
  [LoraConfig reference](https://huggingface.co/docs/peft/v0.19.0/package_reference/lora).

- Ludwig v0.17: [Ludwig docs](https://ludwig.ai/latest/),
  [Ludwig config](https://ludwig.ai/latest/configuration/),
  [GitHub](https://github.com/ludwig-ai/ludwig) — declarative YAML framework
  (Linux Foundation AI & Data, Apache-2.0). Covers SFT, DPO, KTO, ORPO, GRPO
  via `trainer.type`. Advanced PEFT initializers (PiSSA, EVA, CorDA, LoftQ)
  native in config.
