# dme-harness — the Rust direct-model-edit instrument

Operator ruling 2026-11-06: the Python replication workspace is retired and
archived; the live-replication harness is Rust + bash. This crate is that
instrument. The anchor case (MIT CSAIL "Sparks of Weight-Space Programming":
one direction, subtracted from 64 output columns per layer of Qwen3.5-0.8B,
131,072 weights ≈ 0.016%) is replicated with our own measured receipts.

## Modes

| mode | what it does |
|---|---|
| `probe` | measurement leg: N GSM8K-train problems under a card-anchored config, served through the CANONICAL route (the L4 supervisor launches the fleet model per the L1 manifest; completions ride the M3 endpoint semantics with per-row sampling-parity verification), emitted as archived-schema JSONL rows + stage summary |
| `capture` | localization leg (the `engine` feature): per-layer `l_out-N` / `ffn_out-N` activation captures across contrasting trajectories |
| `convert` | embedding-to-edit bridge (pure math, engine-free): embedder vectors + capture manifests -> `u.f32` / `k.f32` / `covdiag.f32` + a sha256-bound manifest + the edit spec; the pre-emit lisp gate (`gates/pre-emit.lisp`) runs over the manifest at decision time |
| `edit` | direction-subtraction edit on a GGUF copy through the shared GGUF writer (Q8_0 minimal-collateral codec; F16/BF16 direct half arithmetic) + edit receipt |
| `gguf-info` | GGUF metadata/tensor dump + sha256 |
| `summary` | recompute a stage summary from a JSONL |

The DME gates are **not** in this crate: DME-0 (config), DME-1 (route),
DME-2 (budget), DME-3 (attribution), DME-4 (convergence) run in `lisp_eval`
at decision time over these receipts — the instrument measures, the gates
decide.

## The routes (no parallel infrastructure)

- **Probe** consumes the private-inference stack (`hkask-private-inference`):
  the L4 supervisor launches the fleet model, the plan is compiled from the
  L1 manifest + L3 placement, and the completions go to
  `LaunchPlan::endpoint()` with the M3 request shape. Default build — no
  in-process engine needed.
- **Capture** builds the PINNED CANONICAL SOURCE via the in-repo
  llama-cpp-sys-4 fork (`kask/crates/llama-cpp-sys-4`, wired by the
  workspace's `[patch.crates-io]`): the published sys crate ships a
  vendored llama.cpp at an unpinned commit and its prebuilt route fails
  closed (`LLAMA_PREBUILT_DIR ... cannot be verified against the active
  llama.cpp patches`) while the decode-hooks patch is staged — so the fork
  takes the source as an env and copies + patches + cmakes the pin
  (commit 9e2a04a) inside its own OUT_DIR: pin-exact, patched, no second
  engine tree, the shared build-hip untouched:
  ```
  LLAMA_CPP_SRC_DIR=$HOME/Documents/zk-data/private-inference/engines/llama-cpp-mainline \
    cargo build -p dme-harness --release --features engine
  ```
- **Edit** and **gguf-info** consume the shared GGUF component
  (`hkask_private_inference::local_leg::{gguf, gguf_writer}`) — this crate
  carries no parser and no codec of its own.

## Build

Default (probe / edit / gguf-info / summary): `cargo build -p dme-harness --release`

With the capture engine (see LLAMA_PREBUILT_DIR above):
`cargo build -p dme-harness --release --features engine`

## The receipts' contract

Rows follow the archived instrument's JSONL schema verbatim (all archived
keys, none renamed or dropped) plus additive instrument keys only:
`harness`, `backend`, `gguf_sha256`, `penalty_last_n`, `n_ctx`,
`engine_version`, `server_generation_settings` (the sampling-parity
receipt — every knob verified against the server's echo, per row).
