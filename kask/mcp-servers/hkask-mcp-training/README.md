# hkask-mcp-training

Model training MCP server — ingests QA pairs and training data for fine-tuning pipelines.

QA generation belongs to the corpus tools in this tree; this server has no
QA-generation caller. Configure `kask.models.qa_generation_model` for corpus's
non-thinking generator (empty by default, explicit tool model takes precedence).
That setting does not replace `training_submit.base_model` or `training_evaluate.model`,
and `HKASK_QA_GENERATION_MODEL` is not injected into this server without a consumer.

Uses internal tool dispatch pattern (not individual `pub async fn` per tool).

## Tools (8)

Simplified from 21 → 15 → 8 across 2026-07-19 cleanups.

| Tool | Description |
|------|-------------|
| `training_ingest_qa` | Ingest QA pairs for model training. Stores question-answer pairs with provenance in semantic memory for future fine-tuning dataset assembly |
| `training_ingest_dataset` | Ingest a raw dataset file into the normalized cache without submitting a training job. Detects format (ChatML, ShareGPT, Alpaca, raw text), normalizes to canonical ChatML, validates, and caches |
| `training_assemble_dataset` | Assemble stored QA pairs into a ChatML JSONL training dataset file. Queries semantic memory for training_qa_pair triples, filters by dataset/source/bloom level, and writes a file ready for training_submit. Optionally splits into train/test |
| `training_submit` | Submit a training job for execution. Ingests, normalizes, and submits a dataset for LoRA fine-tuning via the configured host (axolotl or unsloth). When `feedback_path` is provided, enters retrain mode: merges original + feedback, deduplicates, increments version, pre-registers adapter metadata for A/B comparison |
| `training_status` | Query the status of a training job by its ID. When a job completes, automatically registers the adapter in the persistent store if not already registered |
| `training_cancel` | Cancel a running or queued training job |
| `training_evaluate` | Evaluate a deployed model using exact match, containment, semantic judging with an explicit `judge_model`, or strict benchmark-letter scoring; reports all attempts, error classes, and known/unknown resource use |
| `training_validate_config` | Run the lora-training skill's static math-contract gates (G-M1..G-M4, G-Q1, G-Q2, G-Q4, G-H1) on training params. Also profiles the dataset (G-D0) and validates dataset size (G-D1) if dataset_path is provided. Emits `reg.lora.audit` spans. This is the runtime enforcement point for the `.agents/skills/lora-training/` skill's `audit-config` phase |

### Evaluation contract

`model` selects the deployed candidate; `adapter_id` labels the report and does
not deploy or select an adapter. `semantic` requires a non-empty `judge_model`;
supplying it for another method is rejected. The semantic result is always
`llm_judged`, not an independent ground-truth guarantee. Only trimmed `CORRECT`
and `INCORRECT` verdicts are accepted; other output is an evaluator error.
Unknown methods and `max_examples: 0` are rejected before inference.

Accuracy is `correct / total_examples`, where every attempted example counts,
including generation and evaluator errors. `incorrect`, `generation_errors`,
and `evaluator_errors` partition the non-correct attempts. Per-example `correct`
is null on errors, with explicit `status` and `error`. Blank input lines are
ignored; malformed/invalid rows count in `skipped_invalid_examples`, while
`valid_examples` and `excluded_by_limit` distinguish valid data from capped work.

Benchmark rows require 2–6 non-empty string choices and an available answer
letter A–F. Model output must be exactly one available letter after trimming
and case normalization; prose is not searched for a favorable letter.

Resource totals include candidate and judge calls. `total_tokens_used` and
`total_cost_usd` are null if any call lacks the corresponding report, including
failed calls with unknown consumption. `reported_tokens_used` and
`reported_cost_usd` retain partial sums; `unreported_usage_calls`,
`unreported_cost_calls`, and `inference_calls` describe coverage. A genuinely
reported zero remains zero. These measurements do not enforce a spending cap.

Implementation: `/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-training/src/tools/evaluate.rs`
(`EvaluationSummary`, `training_evaluate`, `eval_benchmark`); behavioral tests
are the `evaluation_*` public-tool fixtures in
`/home/mdz-axolotl/Clones/zed-kask/kask/mcp-servers/hkask-mcp-training/src/hkask_mcp_training.rs`.

### QA dataset assembly

`training_assemble_dataset` reads the server's configured memory store unless
`db_path` selects another database (for example, the database used by corpus QA
ingestion). An explicit `passphrase` takes precedence over the configured DB
passphrase. A missing or incorrect passphrase returns `permission_denied`, not
an internal error or a silent fallback to another key.

Assembly is an export, not recall: neither the default-store path nor the
explicit-DB path updates QA `recalled_at` timestamps. This includes rows excluded
by dataset/source/Bloom filters or the example limit. Filtering, ChatML output,
and optional train/test splitting are unchanged; genuine memory recall APIs
continue to refresh recall clocks.

### Deleted tools (2026-07-19, second pass)

- `training_deploy` / `training_deployment_status` / `training_teardown` — replaced by `hkask_mcp_training::adapter::AdapterPort::{create_endpoint, endpoint_status, teardown_endpoint}`. The MCP server was a thin wrapper; deployment now goes through the canonical AdapterPort surface directly.
- `training_list_adapters` / `training_delete_adapter` — `AdapterPort::list_adapters` and `AdapterStore::delete` already cover these. Rare operations; route via CLI.
- `training_register_adapter` — `training_status` auto-registers on completion; manual registration is an `AdapterStore` API call, not an MCP tool.
- `training_preflight_check` — replaced by `training_validate_config`, which runs the actual lora-training skill gates (not just file-existence checks).
- `training_retrain` — merged into `training_submit` as optional `feedback_path` + `skill_name` + `adapter_name` parameters.

### Deleted tools (2026-07-19, first pass)

- `training_generate_traces`, `training_generate_chain_of_thought` (inference, not training)
- `training_sweep` (use submit in a loop)
- `training_merge_adapters` (speculative, never produced output)
- `training_record_invocation`, `training_curate_feedback` (data curation, not training)
- `training_recommend_model` (can be done offline)

## Gate verification

`training_validate_config` reports the static gates; `training_submit`
rechecks refusal findings before submission. This runtime behavior is distinct
from formal verification. The `#[cfg(kani)]` harnesses in
`src/lora_validation/param_gates.rs` are `gm3_refuse_iff_degenerate_scaling`,
`gm4_findings_follow_rank_thresholds`, `gm1_clean_iff_noop_init`,
`safe_region_has_no_refusals`, and `gm2_warns_iff_bias_breaks_merge`.

The R2 pilot uses Kani **0.68.0**, CBMC **6.11.0**, and Kani's pinned
`nightly-2026-08-21` toolchain. Ordinary Cargo builds do not type-check these
cfg-excluded harnesses. Initial Kani compilation exposed missing `Arbitrary`
support and a use-after-move in the harness. Subsequent runs exhausted memory
while verifying diagnostic allocation/formatting. Production validators now
consume the allocation-free `math_decisions` core; all five harnesses checked
that same core under 2 GiB/120-second bounds with safety/unwinding checks enabled.
Reachability covers passed. A pre-refactor public-tool digest over 1,944
configurations pins findings, messages, sources, remediations, and verdicts;
it is unchanged after extraction. Source-bound results are recorded in
`/home/mdz-axolotl/Clones/zed-kask/kask/docs/plans/goedel-gap-closure-plan.md` §9.
Proofs cover decision outputs/severities, not diagnostic rendering, allocation,
MCP transport, training quality, or PEFT/provider behavior. The extraction is
currently uncommitted; no continuous proof gate or global utility claim exists.

After approved provisioning, run all five with
`bash /home/mdz-axolotl/Clones/zed-kask/kask/scripts/check-bounded-proofs.sh NEW_ARTIFACT_DIRECTORY`.
The runner checks Kani 0.68.0, refuses to overwrite evidence, captures source
hashes/diff, caps each run, and rejects stale source or missing successful
harness summaries. Optional `CARGO_TARGET_DIR` selects the Kani build cache.
Keep unwinding/safety checks enabled; timeout,
unsupported analysis, or resource failure is unknown, not success. These
obligations concern validation predicates, not training quality or useful
self-rewriting. Do not add a crates.io `kani` runtime dependency.

## Providers

Two cloud hosts: **Runpod** (primary, with completion detection via
HuggingFace artifacts) and **Nebius** (no completion detection
yet — `training_status` reports `Running` indefinitely). Two declarative YAML
harnesses are retained: **Axolotl** (SFT) and **Ludwig**
(SFT + DPO/KTO/ORPO/GRPO and advanced PEFT).

Both harnesses use the **same generic Docker image**
(`docker.io/mdzaxo/hkask-training-base:latest`, ~130MB). The harness-specific
packages are pip-installed at pod startup via a dynamically generated install
script (`HKASK_INSTALL_SCRIPT`). No per-harness images.

Harness selection is per-job via `TrainingParams.harness` (operator-accepted
from the lora-training skill's G6 gate), defaulting to Axolotl. The RunPod
host's `submit()` method calls `generate_install_script()` which:
1. Renders the harness-native YAML config
2. Generates a bash install script that pip-installs the harness packages,
   writes the config, runs training, uploads the adapter, and writes the manifest
3. Passes the script to the pod as `HKASK_INSTALL_SCRIPT`

Implemented methods: Axolotl SFT and Ludwig SFT/DPO/KTO/ORPO/GRPO.

Ludwig (Linux Foundation AI & Data, Apache-2.0) is the only harness in the
candidate set covering GRPO (reward-model-free RLHF) and the full advanced-PEFT
initializer set (PiSSA, EVA, CorDA, LoftQ) that hKask's `LoraInit` enum
declares. Source: https://ludwig.ai/latest/ · https://github.com/ludwig-ai/ludwig

Deleted providers (2026-07-19): `TogetherHost` (Together AI REST API). Deleted providers (2026-08-20): the Deep-Infra GPU-container host (provider removed from the repo). The Runpod host is sufficient for all training workloads.


## Configuration

| Variable | Description |
|----------|-------------|
| `RUNPOD_API_KEY` | Runpod API key |
| `RUNPOD_TEMPLATE_ID` | Runpod GPU pod template ID with axolotl pre-installed |
| `RUNPOD_GPU_TYPE_ID` | GPU type override (default: heuristic from model size) |
| `RUNPOD_CONTAINER_DISK_GB` | Container disk size override |
| `RUNPOD_DOCKER_IMAGE` | Docker image override |
| `RUNPOD_DOCKER_ARGS` | Extra Docker args for the pod |
| `HKASK_PODS_FILE` | Path to RunPod pod ID persistence file (default: `data/training-pods.json`) |
| `NEBIUS_PROJECT_ID` | Nebius project ID |
| `NEBIUS_SUBNET_ID` | Nebius subnet ID |
| `NEBIUS_GPU_PLATFORM` | Nebius GPU platform override |
| `NEBIUS_GPU_PRESET` | Nebius GPU preset override |
| `NEBIUS_IMAGE_FAMILY` | Nebius image family override |
| `NEBIUS_CLI_PATH` | Path to Nebius CLI (default: `nebius`) |
| `HF_TOKEN` | HuggingFace token for artifact upload (Runpod path) |
| `HKASK_HF_ARTIFACT_OWNER` | HuggingFace artifact owner (required for Runpod) |
| `HKASK_HF_DATASET_REPO` | HuggingFace dataset repo (required for Runpod) |
| `HKASK_HF_MODEL_REPO` | HuggingFace model repo (required for Runpod) |
| `HKASK_TRAINING_HOST` | Host selector: `runpod` / `nebius` |
| `HKASK_TRAINING_CACHE_DIR` | Cache directory for datasets |
| `HKASK_TRAINING_DB` | Job/adapter SQLite DB path |
| `HKASK_DB_PASSPHRASE` | DB encryption passphrase |
| `HKASK_TEMPLATE_ROOT` | Root for registry templates |
| `HKASK_DATA_DIR` | Data directory (shared with parent process) |

## lora-training skill integration

`training_validate_config` is the runtime enforcement point for the
[`.agents/skills/lora-training/`](../../.agents/skills/lora-training/SKILL.md)
skill's `audit-config` phase. The skill reasons over config files and proposes
regressions; this server enforces the static subset of gates at submit time
and emits the `reg.lora.*` spans the skill's `convergence-check` phase consumes.

## Quick Start

```bash
export RUNPOD_API_KEY="your-key"
export RUNPOD_TEMPLATE_ID="your-template-id"
# The server starts automatically with kask
the zed-kask editor
```
