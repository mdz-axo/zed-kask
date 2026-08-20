---
name: lora-training
description: "LoRA/QLoRA training configuration and contract enforcement for hKask. Produces an advisory PEFT recommendation through a deterministic 8-gate refinement; the operator accepts, overrides, or rejects it, and the runtime enforces hard contracts."
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
  on task requirements, data shape, and — when supplied — the full capability
  space (3 harnesses × 6 trainers × 3 hosts × cost models).
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
- Require `userpod_host` for every action and emit the corresponding registered
  `reg.lora.*` span.

## Instructions

### `lora-training/preflight-dataset`

1. Detect the dataset format from the declared `dataset_path` and check it
   against the expected format for the selected trainer/method. This is the
   runtime-evidence source for G-D0.
2. Emit a three-state verdict: `ready` (use directly), `needs_mapping`
   (compatible but needs column-name preprocessing — mapping code provided),
   or `incompatible` (cannot be used for this method, e.g., SFT data for DPO).
3. When `needs_mapping`, emit copy-paste Python mapping code following the HF
   `dataset_inspector.py` pattern. SFT format conversions (ChatML, ShareGPT,
   Alpaca, RawText) are auto-normalized by the dataset pipeline — no manual
   mapping needed, verdict is `ready`.
4. This phase is optional — skipped when `dataset_path` is absent. It does not
   execute training, load the dataset into memory, or modify files.
5. Emit `reg.lora.preflight`.

### `lora-training/select-method`

1. Read the declared training inputs and preserve explicit operator requirements.
   Consume `prior_iteration` (loop closure), `prior_outcome` (extrinsic
   exploratory experience, τ_t), `prior_operator_feedback` (intrinsic
   evaluative feedback, e_t), `prior_training_history` (Good Regulator), and
   `provider_capabilities` (deep capability reasoning) when supplied.
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
   declared dataset and produce the adapter type implied by G0. When
   `provider_capabilities` is supplied, G6 reasons over the full capability
   space (3 harnesses × 6 trainers × 3 hosts × cost models): available_hosts,
   host_gpu_types, host_cost_models, and inference_provider_capabilities.
   When absent, G6 falls back to harness-method compatibility only. If the
   operator declares `harness_preference` or `trainer_preference` inputs,
   preserve them as `operator_requested` and validate compatibility. If both
   are absent, select based on adapter_purpose and dataset_format_hint. The
   three harnesses have distinct capability profiles:
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
8. The select-method phase is the first turn of a PDCA loop mechanically closed
   by the process manifest's loop step (ordinal 5), which routes
   `convergence_metric`, `blockers`, and `gate_results_summary` back as
   `prior_iteration`. The operator may also revise inputs and re-invoke. The
   loop converges when the convergence metric is ≤ 0.10 and no hard blockers
   remain.
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
6. Apply all 19 gates phase-appropriately: G-M1..G-M5, G-Q1..G-Q6,
   G-D1..G-D3, G-F1..G-F2, G-H1, G-R1 (runtime alert), and G-P1 (persistence
   preflight). Runtime and post-training passes require supplied measurements;
   this template never executes those checks. Consume `dataset_profile` from
   G-D0 for G-D1 dataset size/quality assessment. Consume `runtime_metrics`
   for G-R1 runtime alert assessment (loss spikes, NaN gradients, vanishing
   loss) when supplied. G-P1 verifies HuggingFace artifact persistence is
   configured before submit on ephemeral cloud hosts.
7. Inspect initializer-specific preprocessing and persistence according to the
   selected initializer's documented contract. Do not introduce an EVA-specific
   or framework-version-specific refusal rule.
8. Enforce no-fiction mechanically (v0.31.0): findings with `evidence_kind` of
   `config_value`, `code_presence`, or `code_absence` MUST have non-null
   `evidence.config_path` AND non-null `evidence.line`. Findings that fail this
   check are rejected at the audit gate and counted in `rejected_findings` with
   reason `"missing_citation"`. Findings with `evidence_kind` of
   `not_available`, `operator_assertion`, or `runtime_measurement` are exempt.
9. Emit algedonic escalation (v0.31.0): for every `refuse` finding, emit a
   `refuse_escalation` entry (VSM S1→S5 short-circuit) with `finding_id`,
   `gate_id`, `claim`, `requirement`, `evidence`, `selected_method`,
   `userpod_host`, and `severity: critical`. The escalation is in-addition; the
   manifest and downstream phases still process the finding normally.
10. Emit every result using the normalized Finding schema below, compute readiness
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

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `preflight-dataset.j2` | KnowAct | v0.32.0: Detect dataset format, check compatibility against the expected format for the selected trainer/method, and emit copy-paste Python mapping code when a fixable column-name mismatch is found. Mirrors HF's dataset_inspector.py three-state pattern (Ready / NeedsMapping / Incompatible). Optional — skipped when dataset_path is absent. This is the runtime-evidence source for G-D0. |
| `select-method.j2` | KnowAct | Apply a deterministic 8-gate refinement without overwriting earlier constraints or operator requirements. v0.31.0: G6 reasons over the full capability space (3 harnesses × 6 trainers × 3 hosts × cost models) when provider_capabilities is supplied. G2 and G3 refine using prior_training_history when supplied (Good Regulator compliance). Consumes prior_iteration when present (mechanical PDCA loop closure via manifest). |
| `audit-config.j2` | KnowAct | Read training config, harness, runtime, and post-training evidence. Evaluate the applicable subset of 19 quality gates. v0.31.0: emits refuse_escalation for refuse findings (algedonic S1→S5 short-circuit) and rejects findings with config_value/code_presence/code_absence evidence_kind but null config_path/line (no-fiction enforcement, mechanical not voluntary). Consumes dataset_profile from G-D0 for G-D1 dataset size/quality assessment. v0.32.0: consumes runtime_metrics for G-R1 runtime alert assessment (loss spikes, NaN gradients, vanishing loss) when supplied. v0.32.0: G-P1 persistence preflight verifies HuggingFace artifact persistence is configured before submit on ephemeral cloud hosts. |
| `report.j2` | KnowAct | Synthesize audit findings with concrete config evidence, source citations (arXiv paper sections + PEFT v0.19.0 doc sections), severity (critical/high/medium/low), gate ID, and remediation. Propose RR-NNNN.yaml entries with surface: training for CI-enforced config gates. Preserve the normalized Finding schema, identify contract gaps, and separate recommendation from phase-aware readiness. Produce verdicts from evidence-backed states without reclassifying findings. |

## Constraints

- rJoule cap: 2 per invocation. Maximum 10 iterations.
- The process manifest, registry manifest, and these four `.j2` templates are
  authoritative over this companion. If they conflict, the registry wins.
- All four templates are public. No hidden training controls or parameters.
- Preserve operator sovereignty and authenticated `userpod_host` identity.
- Emit only values, findings, states, citations, and measurements supported by
  declared evidence. Do not invent defaults, snippets, line numbers, benchmark
  results, training outcomes, or regression counts.
- No-fiction enforcement is mechanical (v0.31.0): findings with
  `config_value`/`code_presence`/`code_absence` evidence_kind and null
  `config_path`/`line` are rejected at the audit gate, not merely discouraged.
- Algedonic escalation (v0.31.0): `refuse` findings emit `refuse_escalation`
  in-addition to normal flow so safety-boundary violations reach the operator
  before the full pipeline completes.
- Convergence honesty (v0.31.0): `not_evaluated` maps to risk 0.5 (coverage
  gap), distinct from `deferred`/`planned` at 1.0 (known unmet requirement).
  Critical/high contribution is graded (0→0.6→0.8→1.0), not binary.
- Runtime and post-training gates are requirements or assessments of supplied
  measurements; the skill does not execute them.
- Regression proposals are human-reviewed, `status: pending`, and
  `surface: training`.
- Self-improvement feedback loop (v0.31.0): the runtime emits
  `reg.skill.lora-training.outcome` and `reg.skill.lora-training.operator_feedback`
  spans when training completes/fails or the operator reacts to a recommendation.
  These become `prior_outcome` (τ_t) and `prior_operator_feedback` (e_t) on
  subsequent invocations.
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
