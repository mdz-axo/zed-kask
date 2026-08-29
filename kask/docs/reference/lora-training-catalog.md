---
title: "LoRA Training — Method & Gate Catalog"
audience: [developers, ml-engineers]
last_updated: 2026-08-28
version: "0.39.0"
status: "Active"
domain: "Training"
mds_categories: [domain, trust]
---

# LoRA Training — Method & Gate Catalog

Reference catalog for the `lora-training` skill
(`.agents/skills/lora-training/SKILL.md`, 315 lines) and its runtime
enforcement point, the `hkask-mcp-training` MCP server
(`kask/mcp-servers/hkask-mcp-training/`). The registry templates
(`kask/registry/templates/lora-training/`: `select-method.j2`,
`audit-config.j2`, `preflight-dataset.j2`, `report.j2`) remain authoritative
(P5.1); this document is a derived reference.

## MCP Server Surface (9 tools)

`hkask-mcp-training` exposes 9 tools, each tagged with an ML-Schema ontology
concept via `ontology_anchor` (`kask/mcp-servers/hkask-mcp-training/src/hkask_mcp_training.rs:314-326`):

| Tool | Ontology anchor | Role |
|---|---|---|
| `training_ingest_dataset` | `mls:Data` | Dataset ingestion |
| `training_ingest_qa` | `mls:Data` | QA-pair ingestion |
| `training_assemble_dataset` | `mls:Data` | Dataset assembly |
| `training_submit` | `mls:Run` | Submit a training job |
| `training_status` | `mls:Run` | Job status (consumes completion-manifest `runtime_metrics` for G-R1) |
| `training_cancel` | `mls:Run` | Cancel a job |
| `training_evaluate` | `mls:Model` | Post-training evaluation |
| `training_validate_config` | `mls:Model` | Runtime enforcement point for the skill's `audit-config` phase |
| `training_bridge_rollouts` | `mls:Run` | Rollout-harness bridge (`src/tools/rollout_bridge.rs:54-55`) |

`training_validate_config` is the runtime enforcement point: the skill reasons
over config files and proposes regressions; the server enforces the static
subset of gates at submit time and emits the `reg.lora.*` spans the skill's
convergence-check phase consumes
(`hkask_mcp_training.rs:53-58`). Host selection: RunPod is the only cloud
host; the harness default is Axolotl, with per-job harness selection honored
at submit time (`hkask_mcp_training.rs:42-48`).

## Method Catalog

| Method | When (gate) | Init | Mergeable | Source |
|--------|-------------|------|-----------|--------|
| LoRA | G4=default | `B=0`, `A~Gaussian` | Yes | arXiv:2106.09685 |
| QLoRA | G2=memory-bound | LoRA init on NF4 base | Yes (after dequant) | arXiv:2305.14314 |
| rsLoRA | G3=r>64 | LoRA init, `α/√r` scaling | Yes | arXiv:2312.03732 |
| DoRA | G4=cost-sensitive, low r | LoRA init + magnitude | Yes (PEFT ≥0.10) | arXiv:2402.09353 |
| PiSSA | G4=fast convergence | SVD of base weight | Yes (via `subtract_mutated_init`) | arXiv:2404.02948 |
| LoRA-GA | G4=fast convergence | Gradient SVD | Yes (via `save_mutated_as_lora`) | arXiv:2407.05000 |
| CorDA-KP | G5=knowledge preservation | Context-oriented | Yes | PEFT v0.19.0 `corda_config` |
| EVA | G4=data-driven init | SVD of activations from training data | Yes | EVA arXiv:2410.07170; PEFT v0.19.0 `eva_config` |
| AdaLoRA | (not default) | SVD-parameterized | Yes | arXiv:2303.10512 |
| aLoRA | G1=dynamic-switching | LoRA init | No (selective) | PEFT v0.19.0 `alora_invocation_tokens` |
| IA³ | (extreme param budget) | Vector scaling | Yes | PEFT `IA3Config` |
| Prefix Tuning | (not recommended for production) | Prefix vectors | No | PEFT `PrefixTuningConfig` |

New methods can be added as PEFT exposes them and literature justifies
them (P7) — not speculatively.[^peft-catalog]

## Harness Capability Matrix

Three harnesses are supported. Each has a distinct capability profile.

| Capability | Axolotl | TRL | Ludwig |
|---|---|---|---|
| Config format | YAML | Python script | YAML (declarative) |
| SFT | ✅ | ✅ (SFTTrainer) | ✅ (trainer.type: finetune) |
| DPO | ✅ (rl: dpo) | ✅ (DPOTrainer) | ✅ (trainer.type: dpo) |
| KTO | ✅ (rl: kto) | ✅ (KTOTrainer) | ✅ (trainer.type: kto) |
| ORPO | ✅ (rl: orpo) | ✅ (ORPOTrainer) | ✅ (trainer.type: orpo) |
| Reward modeling | ✅ (rl: reward_model) | ✅ (RewardTrainer) | ❌ |
| GRPO (reward-model-free RLHF) | ✅ (rl: grpo) | ✅ (GRPOTrainer, requires vLLM) | ✅ (trainer.type: grpo) |
| Advanced PEFT initializers (PiSSA, CorDA, LoftQ) | ✅ (via peft_init_lora_weights) | ✅ (via PEFT) | ✅ (native in config) |
| EVA initializer | ✅ (via peft_init_lora_weights) | ✅ (via PEFT) | ✅ (native in config) |
| assistant_only_loss | ❌ | ✅ | ❌ |
| Packing strategies (bfd/bfd_split/wrapped) | ✅ (sample_packing) | ✅ | ❌ |
| VLM support | ✅ | ✅ | ✅ |
| Chunked cross-entropy | ✅ (cut_cross_entropy) | ✅ | ❌ |
| Full fine-tuning | ✅ (adapter: qlora or none) | ✅ | ✅ |
| Runtime default | ✅ (when harness=undetermined) | ❌ | ❌ |

Harness selection is driven by the G6 gate (harness capability) in the
`select-method` phase. The operator accepts, overrides, or rejects the
recommendation. The runtime enforces harness-method compatibility via G-H1
(`kask/mcp-servers/hkask-mcp-training/src/lora_validation/param_gates.rs:429-500`).[^trl-catalog][^ludwig-catalog]

## Gate Catalog

19 phase-aware contract gates enforced across the `select-method` and
`audit-config` phases, plus the 8-gate recommendation refinement in
`select-method` (G0, G-D0, G1-G6). Each gate is a single assertion with a
citation.[^lora-contract-gates]

### Recommendation Gates (select-method phase)

| Gate | ID | Purpose | Source |
|------|----|---------|--------|
| Adapter purpose | G0 | Establishes what kind of adapter is being produced (instruction, reasoning, vision, preference, reward_model). Determines baseline rank, target modules, and learning-forgetting posture. Runs first, constrains all subsequent gates. | Biderman et al. arXiv:2405.09673; Raschka 2025 |
| Dataset analysis | G-D0 | Probes the actual dataset file to derive format, sample count, content length stats, token estimates, role distribution, multi-turn detection, vision data detection, and preference pair balance. Feeds into G0, G3, G6. Best-effort — falls back to declared inputs if unavailable. Runtime-evidence source: `preflight-dataset.j2`. | QLoRA §5; TRL dataset formats |
| Inference constraint | G1 | Must-merge vs dynamic-switching vs either-ok. Constrains adapter form. | LoRA §4.2 |
| Memory budget | G2 | Full precision vs quantized 4bit. Model_size_b × 2 as approximate floor only. | QLoRA §3 |
| Task distance | G3 | Refines rank_range within G0 baseline. Light/moderate/heavy. | LoRA §4.3; Biderman et al. |
| Quality vs cost | G4 | Refines adapter_form and/or initializer. | LoRA §4.1; PiSSA; LoRA-GA; DoRA |
| Knowledge preservation | G5 | Refines preservation and initializer (CorDA-KP). | PEFT corda_config; Razin et al. |
| Harness capability | G6 | Selects harness based on training approach from G0-G5. Harness must process dataset and produce adapter type from G0. | TRL; Ludwig; Axolotl |

### Math-Contract Gates (from LoRA paper, arXiv:2106.09685)

| Gate | ID | Assertion | Source |
|------|----|-----------|--------|
| Initialization invariant | G-M1 | Default LoRA and EVA are no-ops at step 0 because B=0. Preserve the operator-selected initializer; evaluate configuration and runtime evidence in their applicable phases. | LoRA §4.1; EVA arXiv:2410.07170; PEFT `init_lora_weights` |
| Merge equivalence | G-M2 | `W_merged = W + (α/r)·BA` ≡ `W + adapter` within fp tolerance. Broken by `bias='all'`/`'lora_only'`, DoRA on PEFT<0.10. | LoRA §4.2; PEFT `bias` docstring |
| Scaling form | G-M3 | scaling = `α/r` (or `α/√r` if `use_rslora`). Never `α`, `r/α`, or `1`. | LoRA §4.1; rsLoRA arXiv:2312.03732 |
| Rank budget | G-M4 | `r < min(d_in, d_out)`. Warn if `r ≥ 0.5×min`. Refuse if `r ≥ min`. | LoRA §4.3 |
| Trainable param count | G-M5 | `2·r·(d_in+d_out)·n_layers` matches `print_trainable_parameters()` within 1%. | LoRA Table 5 |

### QLoRA-Specific Gates (from QLoRA paper, arXiv:2305.14314)

Only apply if QLoRA mode selected (G2).

| Gate | ID | Assertion | Source |
|------|----|-----------|--------|
| Frozen base quantized | G-Q1 | `base_param.dtype` is 4-bit NF4. | QLoRA §3 |
| Adapter dtype | G-Q2 | LoRA `A`, `B` are bf16/fp32, never 4-bit. | QLoRA §3 |
| Gradient flow | G-Q3 | `A.grad` and `B.grad` are not None after first backward. | QLoRA §3 |
| No silent upcast | G-Q4 | Frozen base not cast to fp32 by `prepare_model_for_kbit_training`. | QLoRA §3; PEFT docstring |
| Paged optimizer (conditional) | G-Q5 | `paged_adamw_8bit` only required if peak memory > VRAM. | QLoRA §3 |
| NF4 optimality assumption | G-Q6 | NF4 optimal for normally-distributed weights; warn if non-normal. | QLoRA §3 |

### Data/Eval Gates (from QLoRA paper, arXiv:2305.14314)

| Gate | ID | Assertion | Source |
|------|----|-----------|--------|
| Dataset size vs quality | G-D1 | `n<1000` requires justification; `n>100000` requires quality audit. | QLoRA §5 |
| Eval protocol | G-D2 | Vicuna/MMLU alone not trustworthy; require human or GPT-4 eval. | QLoRA §5 |
| Lemon-pick analysis | G-D3 | Report failure cases, not just aggregate score. | QLoRA §6 |

### Forgetting Gates (post-QLoRA literature)

| Gate | ID | Assertion | Source |
|------|----|-----------|--------|
| Intruder dimension check | G-F1 | Report intruder dimensions before/after training via `reduce_intruder_dimension`. | Razin et al. arXiv:2410.21228 |
| Knowledge preservation (CorDA) | G-F2 | If CorDA Knowledge-Preserved mode: assert world-knowledge eval doesn't regress. | CorDA; PEFT `corda_config` docstring |

### Harness Gates

| Gate | ID | Assertion | Source |
|------|----|-----------|--------|
| Harness-method compatibility | G-H1 | Selected harness supports the selected method/trainer. axolotl=SFT/DPO/KTO/ORPO/GRPO/GDPO/RM/FullFT (via `rl:`); trl=SFT/DPO/KTO/ORPO/Reward; ludwig=SFT/DPO/KTO/ORPO/GRPO + advanced PEFT initializers. `trl_trainer` is TRL-specific — warn (not refuse) when set with axolotl or ludwig. Runtime enforcement: `validate_harness_compatibility` (`param_gates.rs:444-500`). | Axolotl — https://docs.axolotl.ai/docs/rlhf.html; TRL — huggingface.co/docs/trl/index; Ludwig — ludwig.ai/latest/configuration/ |

### Runtime Gates

| Gate | ID | Assertion | Source |
|------|----|-----------|--------|
| Runtime alert | G-R1 | Consumes `runtime_metrics` from the completion manifest (loss, grad_norm, alerts) during `training_status`. Flags loss spikes, NaN gradients, vanishing loss. `deferred` when `runtime_metrics` absent; `not_applicable` in preflight. | PEFT training diagnostics; hKask v0.32.0 |
| Persistence preflight | G-P1 | Verifies HuggingFace artifact persistence is configured before training starts. Checks `HF_TOKEN` presence and write access to the target repo. Refuses if persistence is required but unconfigured. | HuggingFace Hub API; hKask v0.32.0 |

## Convergence

The `select-method` phase is the first turn of a PDCA loop closed by
re-entering the cycle, which routes `convergence_metric`, `blockers`, and
`gate_results_summary` back as `prior_iteration`
(`kask/registry/templates/lora-training/select-method.j2:122`; skill
SKILL.md:135-140). **The loop converges when the convergence metric is
≤ 0.10 and no hard blockers remain** (`select-method.j2:195`; skill
SKILL.md:139-140). The operator may also revise inputs and re-invoke.

> **Provenance note:** an earlier revision of this document carried a
> weighted-dimension rubric (0.35/0.20/0.15/0.10) and Cauchy-criterion
> parameters (`cauchy_epsilon: 0.03`, `cauchy_window: 3`). Those specifics
> appear nowhere in the current skill, templates, or server code and have
> been removed as unanchored. The verified contract is the threshold rule
> above.

## Footnotes

[^peft-catalog]: Liu, Y., et al. (2024). *Parameter-Efficient Fine-Tuning (PEFT)*. Hugging Face. https://huggingface.co/docs/peft
    Cited for the PEFT library that exposes the adapter methods the catalog tracks.

[^trl-catalog]: Hugging Face. (2024). *TRL: Transformer Reinforcement Learning*. https://huggingface.co/docs/trl
    Cited for the TRL harness the G6 gate selects when SFT/DPO/KTO/ORPO/Reward training is needed.

[^ludwig-catalog]: Ludwig. (2024). *Ludwig: Declarative Deep Learning Framework*. Linux Foundation AI & Data. https://ludwig.ai/latest/
    Cited for the Ludwig harness the G6 gate selects for declarative YAML-configured training.

[^lora-contract-gates]: Hu, E. J., Shen, Y., Wallis, P., Allen-Zhu, Z., Li, Y., Wang, S., Wang, L., & Chen, W. (2021). LoRA: Low-Rank Adaptation of Large Language Models. arXiv. https://arxiv.org/abs/2106.09685
    Cited as the primary source the math-contract gates (G-M1 through G-M5) derive their assertions from.

## Source References

- **LoRA:** arXiv:2106.09685 — arxiv.org/abs/2106.09685
- **QLoRA:** arXiv:2305.14314 — arxiv.org/abs/2305.14314
- **rsLoRA:** arXiv:2312.03732 — arxiv.org/abs/2312.03732
- **DoRA:** arXiv:2402.09353 — arxiv.org/abs/2402.09353
- **PiSSA:** arXiv:2404.02948 — arxiv.org/abs/2404.02948
- **LoRA-GA:** arXiv:2407.05000 — arxiv.org/abs/2407.05000
- **AdaLoRA:** arXiv:2303.10512 — arxiv.org/abs/2303.10512
- **Razin et al. (intruder dimensions):** arXiv:2410.21228 — arxiv.org/abs/2410.21228
- **Biderman et al. (LoRA Learns Less and Forgets Less):** arXiv:2405.09673 —
  arxiv.org/abs/2405.09673. LoRA underperforms full FT on code/math at low rank;
  high rank (r=256) matches full FT on IFT but not CPT. LoRA forgets less —
  a feature for knowledge preservation. Rank is the learning-forgetting knob.
- **Thinking Machines Lab (LoRA Without Regret):** thinkingmachines.ai/blog/lora —
  For SFT on small-to-medium instruction/reasoning datasets, LoRA performs the
  same as full FT. For datasets exceeding LoRA capacity, LoRA underperforms.
  Learning rate tuning is critical.
- **AutoPEFT (rejected alternative):** arXiv:2301.12132 — arxiv.org/abs/2301.12132
- **EVA:** arXiv:2410.07170 — arxiv.org/abs/2410.07170
- **DPO:** arXiv:2305.18290 — arxiv.org/abs/2305.18290
- **KTO:** arXiv:2402.01306 — arxiv.org/abs/2402.01306
- **ORPO:** arXiv:2403.07691 — arxiv.org/abs/2403.07691
- **PEFT v0.19.0:** huggingface.co/docs/peft/v0.19.0/package_reference/lora
- **TRL v1.8.0:** huggingface.co/docs/trl/index — SFTTrainer, DPOTrainer, KTOTrainer, ORPOTrainer, RewardTrainer
- **Ludwig v0.17:** ludwig.ai/latest/ — declarative YAML deep-learning framework
  (Linux Foundation AI & Data, Apache-2.0). Covers SFT, DPO, KTO, ORPO, GRPO
  via trainer.type. Advanced PEFT initializers (PiSSA, EVA, CorDA, LoftQ)
  native in config. github.com/ludwig-ai/ludwig.
- **GRPO:** arXiv:2402.03300 — Group Relative Policy Optimization
  (reward-model-free RLHF). Implemented in Ludwig via trainer.type: grpo;
  TRL's online RL trainers are deferred (require vLLM co-location).
- **Practitioner consensus:** Raschka (magazine.sebastianraschka.com), Brenndoerfer (mbrenndoerfer.com), Spheron (spheron.network/blog/peft-methods-2026-dora-galore-pissa-vera-guide), Databricks (databricks.com/blog/efficient-fine-tuning-lora-guide-llms), Gradient Flow (gradientflow.com/lora-or-full-fine-tuning/)
