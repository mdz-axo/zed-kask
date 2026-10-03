---
title: "Training MCP Server Reference"
audience: [developers, architects, agents]
last_updated: 2026-10-01
version: "0.42.0"
status: "Active"
domain: "Composition"
mds_categories: [domain, composition, trust, lifecycle]
---

# Training MCP Server Reference

**Crate:** `kask/mcp-servers/hkask-mcp-training`
**Tools:** 9 — `training_ingest_qa`, `training_ingest_dataset`, `training_assemble_dataset`, `training_validate_config`, `training_submit`, `training_status`, `training_cancel`, `training_evaluate`, `training_bridge_rollouts`
**Auto-start:** Yes by default with the full built-in set; `kask.mcp.load_default=false` disables the fleet and `kask.mcp.overrides.training=false` disables this server (server id `training`, `kask/crates/kask_bridge/src/mcp_servers.rs:427`).

The training server is the **LoRA/QLoRA training job surface**: dataset
ingestion and assembly, config validation against the math-contract gates,
job submission, status/cancel, and deployed-adapter evaluation. The
register's L18 row classifies its loop (operator-confirmed submit +
polling); training runs on cloud hosts — there is no local training path.

## The consent gate (P2)

`training_submit` (`tools/submit.rs:28`) requires `confirmed: true` — the
human-in-the-loop GPU-spend gate. The refusal text names the contract:
the operator must see the estimated cost and explicitly approve before
`confirmed` is set (`tools/submit.rs:46-50`). A submission without it is
rejected, never silently queued.

## Tool groups

- **Datasets** — `training_ingest_qa` (`tools/dataset.rs:18`, QA pairs into
  the normalized cache), `training_ingest_dataset` (`:198`, raw JSONL/JSON/
  TXT), `training_assemble_dataset` (`:73`, the ChatML assembly with the
  train/test split).
- **Validation and submission** — `training_validate_config`
  (`tools/validate.rs:15`, the lora-training skill's G-M1..G-Q gates:
  no-op-at-init, merge equivalence, scaling form, rank budget, frozen-base
  quantization), `training_submit` (`tools/submit.rs:28`, the consent-gated
  job submission), `training_status` (`tools/status.rs:17`),
  `training_cancel` (`tools/cancel.rs:13`).
- **Evaluation and evidence** — `training_evaluate`
  (`tools/evaluate.rs:97`, deployed adapter against a test set: exact-match,
  contains, semantic with a named judge, or benchmark), and
  `training_bridge_rollouts` (`tools/rollout_bridge.rs:48`, verdict-labeled
  rollouts into training datasets — SFT from passed, preference pairs from
  passed+failed).

## Architecture

- **Store**: `HKASK_TRAINING_DB` overrides the per-agent default under the
  hKask data dir (`mcp_server_db("training", "training")`, D28-pinned by
  `default_db_path_follows_standardized_layout`,
  `hkask_mcp_training.rs:545`). Job, adapter, and dataset state share the
  one SQLCipher database.
- **Credentials**: `HKASK_DB_PASSPHRASE` declared **optional** — and the
  declaration is honest: on resolution failure the server warns ("Falling
  back to in-memory DB; job/adapter state will not persist across
  restarts", `hkask_mcp_training.rs:403-409`) and serves from memory. The
  degradation is surfaced with its consequence named, never silent — the
  deliberate contrast with the start-refusing durable stores (curator,
  experimentation, kata-kanban): training jobs are one-shot, so an ephemeral
  store is a usable degraded mode, not a broken record path.
- **Hosts and providers**: RunPod and Nebius — `RUNPOD_API_KEY`,
  `NEBIUS_PROJECT_ID`, `NEBIUS_SUBNET_ID` optional, required only when the
  matching host is used (`hkask_mcp_training.rs:519-527`);
  `RUNPOD_TEMPLATE_ID` reads the config-env path with the default image as
  its real fallback (`:528-532`).

## Testing

The tool suites drive each tool through its parameter seams
(`src/tools/`); the D28 path pin and the credential-declaration form are
pinned in-crate. The lora-training skill's gate math (G-M1..G-Q5) is the
validation contract `training_validate_config` enforces.

## Reference models

`kask/docs/reference/lora-training-catalog.md` is the recorded reference
doc (the anchor ledger's L18 PARTIAL row — the `lora-training` skill
itself lacks a `## Reference models` section, a recorded gap); the
lora-training skill governs the advisory PEFT recommendation the gates
produce.