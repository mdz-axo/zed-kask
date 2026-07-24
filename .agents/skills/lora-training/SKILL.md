---
name: lora-training
visibility: public
description: >
  LoRA/QLoRA training configuration and contract enforcement skill for hKask
  (v0.31.0). Produces an advisory, composable PEFT recommendation through a
  deterministic 8-gate refinement (adapter purpose → dataset analysis →
  memory → task distance → quality/cost → knowledge preservation → harness
  capability); the operator accepts, overrides, or rejects it, and the runtime
  enforces established hard contracts against concrete accepted configuration.
  Training approach selection (G0-G5) precedes harness selection (G6) — the
  harness is selected based on its capability to efficiently process the
  declared dataset and produce the adapter type implied by G0. Audits math,
  quantization, data/evaluation, forgetting, and harness-method compatibility
  gates with phase-aware states and evidence. PDCA iteration loop:
  select-method → audit-config → convergence-check → revise → re-invoke.
  Emits reg.lora.* spans.
---

# LoRA Training

Recommend a composable PEFT configuration from declared evidence, audit the
operator-selected configuration without replacing it, report normalized
findings losslessly, and compute phase-aware training-readiness convergence.
This skill does not train, load, initialize, merge, or evaluate models.

## When to Use

- Before training, to obtain an evidence-grounded PEFT recommendation while
  preserving explicit operator requirements.
- After the operator accepts, overrides, or rejects that recommendation, to
  audit the selected concrete configuration and declared harness.
- When runtime or post-training measurements are supplied, to assess established
  contracts without fabricating execution results.
- To report training findings, readiness, contract gaps, and evidence-backed
  `surface: training` regression proposals.
- To compute convergence for the current lifecycle phase and expose preflight,
  runtime-contract, and post-training posture separately.
- To recommend a training harness (Axolotl, TRL, or Ludwig) and trainer based
  on task requirements and data shape.

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
- Require `userpod_host` for every action and emit the corresponding registered
  `reg.lora.*` span.

## Instructions

### `lora-training/select-method`

1. Read the declared training inputs and preserve explicit operator requirements.
2. Refine one composable recommendation record through eight gates: adapter
   purpose (G0), dataset analysis (G-D0), inference constraint (G1), memory
   evidence (G2), task distance (G3), quality/cost (G4), knowledge
   preservation (G5), and harness capability (G6). Training approach
   selection (G0-G5) precedes harness selection (G6). Gates refine
   compatible fields; they do not overwrite the whole recommendation or
   silently replace earlier constraints.
3. Emit only derivable values for `adapter_purpose`, `base_mode`, `adapter_form`,
   `scaling`, `initializer`, `preservation`, `rank_range`,
   `target_module_strategy`, `harness`, and `trainer`; otherwise emit
   `undetermined`, required evidence, alternatives, constraints, or conflicts.
4. Treat `model_size_b × 2` only as an approximate bf16 base-weight floor.
   Memory pressure may favor QLoRA, but these two scalar inputs do not establish
   that a configuration fits or will OOM.
5. Preserve operator-requested initializers uniformly. For EVA, report
   `initialize_lora_eva_weights(model, dataloader)` as required evidence; do not
   hardcode a recommendation-phase refusal.
6. G0 (adapter purpose) establishes what kind of adapter is being produced
   (instruction, reasoning, vision, preference, reward_model). This
   determines baseline rank ranges, target module strategies, and the
   learning-forgetting tradeoff posture. G0 runs first and constrains all
   subsequent gates.
   G-D0 (dataset analysis) runs alongside G0. If `dataset_path` is declared,
   the skill requests the runtime to profile the actual dataset file via
   `training_validate_config`. The profile includes: format detection, sample
   count, content length statistics, token estimates, role distribution,
   multi-turn detection, vision data detection, and preference pair balance.
   The profile feeds into G3 (rank refinement), G6 (harness selection), and
   the adapter_purpose inference. If the profile is unavailable, the skill
   falls back to `dataset_format_hint` and declared inputs.
7. G6 (harness capability) selects a harness based on the training approach
   determined by G0-G5. The harness must be able to efficiently process the
   declared dataset and produce the adapter type implied by G0. If the operator
   declares `harness_preference` or `trainer_preference` inputs, preserve them
   as `operator_requested` and validate compatibility. If both are absent,
   select based on adapter_purpose and dataset_format_hint. The three harnesses
   have distinct capability profiles:
   - **Axolotl** (YAML, SFT + DPO + KTO + ORPO + GRPO + GDPO + RM + Full FT):
     mature, single-file config, the runtime default for instruction adapters.
     Uses `rl:` parameter for preference tuning and GRPO. Supports advanced PEFT
     initializers via `peft_init_lora_weights`. Cannot render TRL-specific
     trainers (trl_trainer is ignored — warn, not refuse).
   - **TRL** (Python, SFT + preference): HF-native, supports SFTTrainer,
     DPOTrainer, KTOTrainer, ORPOTrainer, RewardTrainer. Best for
     assistant_only_loss, packing strategies, VLMs, and preference
     optimization from paired/unpaired data.
   - **Ludwig** (YAML, SFT + preference + GRPO): declarative like axolotl,
     but covers the full alignment spectrum including GRPO
     (reward-model-free RLHF) and advanced PEFT initializers (PiSSA, EVA,
     CorDA, LoftQ) that axolotl cannot render. Best when the operator needs
     GRPO or an initializer axolotl doesn't support.
   Axolotl remains the runtime default when harness is undetermined and
   adapter_purpose is instruction — no silent migration. For non-instruction
   purposes, axolotl is not a valid default.
8. The select-method phase is the first turn of a PDCA loop. After audit-config
   and convergence-check, the operator may revise inputs and re-invoke.
   Each iteration refines the recommendation. The loop converges when the
   convergence metric is ≤ 0.10 and no hard blockers remain.
9. Return separate `recommendation`, `readiness`, `justification`, and
   `authority` objects. Emit `reg.lora.select`.

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
6. Apply all 17 gates phase-appropriately: G-M1..G-M5, G-Q1..G-Q6,
   G-D1..G-D3, G-F1..G-F2, and G-H1. Runtime and post-training passes require
   supplied measurements; this template never executes those checks.
7. Inspect initializer-specific preprocessing and persistence according to the
   selected initializer's documented contract. Do not introduce an EVA-specific
   or framework-version-specific refusal rule.
8. Emit every result using the normalized Finding schema below, compute readiness
   separately, and emit `reg.lora.audit` for every represented gate.

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
- `userpod_host`

Do not create alternate finding shapes. A recommendation never overwrites
`selected_method`, and unavailable evidence never becomes an observed violation.

### `lora-training/report`

1. Validate `userpod_host` and consume normalized findings without adding,
   removing, renaming, repairing, or reclassifying fields.
2. Present complete findings unchanged; grouped views may organize them by phase,
   state, or severity only.
3. Report counts for all eight states and four phases. Keep selected method,
   advisory method recommendations, and readiness separate.
4. Record `deferred`, `planned`, and `not_evaluated` requirements as contract
   gaps with the next evidence needed; exclude `not_applicable`. Do not mutate
   findings to create gaps.
5. Propose `status: pending`, `surface: training` regressions only from eligible,
   concretely evidenced `fail`/`refuse` findings, or policy-permitted `warn`
   findings. Never propose one solely from unavailable evidence or an unevaluated
   state.
6. Derive readiness with precedence:
   `Refuse > Fail > Conditional > Deferred > Not evaluated > Pass`.
   A different method recommendation cannot change the verdict.
7. Preserve claim-appropriate citations and emit `reg.lora.report` with exact
   phase, state, severity, and evidence-kind counts.

### `lora-training/convergence-check`

1. Accept `current_phase` (`preflight | runtime | post_training`) and current
   evidence only. Reject unknown gate states as blockers.
2. Compute risk over currently applicable dimensions: critical/high findings
   (0.40), math gates (0.25), QLoRA gates when applicable (0.15), data/eval
   gates (0.10), and forgetting gates when applicable (0.10). Exclude
   non-applicable gates and empty families, then normalize the remaining weights.
3. Map gate risk as `pass=0`, `warn=0.5`, and
   `fail/refuse/deferred/planned/not_evaluated=1`. Future- and past-phase gates
   do not enter the current metric denominator.
4. Set `converged=true` only when the normalized metric is `≤ 0.10` and no hard
   blocker exists. A stable metric below threshold remains converged; a 5%
   improvement is diagnostic only, not required.
5. Return phase-aware outputs: `preflight_ready`,
   `runtime_contracts_pending`, and `post_training_verified`, plus blockers and
   a reproducible gate-results summary. These do not replace the current-phase
   `converged` verdict.
6. Emit `reg.lora.convergence` unconditionally.

## Registry Templates

| Template | Type | Purpose |
|---|---|---|
| `select-method.j2` | `KnowAct` | Produce an advisory composable recommendation via eight-gate refinement (G0 adapter purpose, G-D0 dataset analysis → G1-G5 method → G6 harness), explicit uncertainty, operator authority, PDCA iteration loop, and runtime enforcement boundaries. |
| `audit-config.j2` | `KnowAct` | Audit declared artifacts with phase-aware gates, states, evidence kinds, normalized findings, and separate readiness. |
| `report.j2` | `KnowAct` | Preserve findings losslessly; report readiness and contract gaps; propose only evidence-backed pending regressions. |
| `convergence-check.j2` | `KnowAct` | Compute normalized current-phase convergence and preflight/runtime/post-training posture from supplied evidence. |

## Constraints

- The registry manifest and these four `.j2` templates are authoritative over
  this companion.
- All four templates are public. No hidden training controls or parameters.
- Preserve operator sovereignty and authenticated `userpod_host` identity.
- Emit only values, findings, states, citations, and measurements supported by
  declared evidence. Do not invent defaults, snippets, line numbers, benchmark
  results, training outcomes, or regression counts.
- Runtime and post-training gates are requirements or assessments of supplied
  measurements; the skill does not execute them.
- Regression proposals are human-reviewed, `status: pending`, and
  `surface: training`.
- `kali-audit` owns security findings; `tdd` owns training-loop code correctness;
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
- TRL v1.8.0:
  [SFTTrainer](https://huggingface.co/docs/trl/main/en/sft_trainer),
  [DPOTrainer](https://huggingface.co/docs/trl/main/en/dpo_trainer),
  [KTOTrainer](https://huggingface.co/docs/trl/main/en/kto_trainer),
  [ORPOTrainer](https://huggingface.co/docs/trl/main/en/orpo_trainer),
  [RewardTrainer](https://huggingface.co/docs/trl/main/en/reward_trainer),
  [TRL index](https://huggingface.co/docs/trl/index).
- Ludwig v0.17: [Ludwig docs](https://ludwig.ai/latest/),
  [Ludwig config](https://ludwig.ai/latest/configuration/),
  [GitHub](https://github.com/ludwig-ai/ludwig) — declarative YAML framework
  (Linux Foundation AI & Data, Apache-2.0). Covers SFT, DPO, KTO, ORPO, GRPO
  via `trainer.type`. Advanced PEFT initializers (PiSSA, EVA, CorDA, LoftQ)
  native in config.
