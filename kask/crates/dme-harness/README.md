# dme-harness — the Rust direct-model-edit instrument

Operator ruling 2026-11-06: the Python replication workspace is retired and
archived; the live-replication harness is Rust + bash. This crate is that
instrument. The anchor case (MIT CSAIL "Sparks of Weight-Space Programming":
one direction, subtracted from 64 output columns per layer of Qwen3.5-0.8B,
131,072 weights ≈ 0.016%) is replicated with our own measured receipts.

## Modes

| mode | what it does |
|---|---|
| `probe` | measurement leg: N GSM8K-train problems under a card-anchored config, emitted as archived-schema JSONL rows + stage summary |
| `capture` | localization leg: per-layer `l_out-N` / `ffn_out-N` activation captures across contrasting trajectories |
| `edit` | direction-subtraction edit on a GGUF copy (Q8_0 minimal-collateral codec; F16/BF16 direct half arithmetic) + edit receipt |
| `gguf-info` | GGUF metadata/tensor dump + sha256 |
| `summary` | recompute a stage summary from a JSONL |

The DME gates are **not** in this crate: DME-0 (config), DME-1 (route),
DME-2 (budget), DME-3 (attribution), DME-4 (convergence) run in `lisp_eval`
at decision time over these receipts — the instrument measures, the gates
decide.

## Build

Default build (no inference engine — `edit` and `gguf-info` work):
`cargo build -p dme-harness --release`

With the inference engine (CPU):
`cargo build -p dme-harness --release --features engine`

With the Vulkan backend on this machine the vendored llama.cpp needs
SPIRV-Headers, which is installed at a local prefix (the upstream
`ggml-vulkan` CMakeLists never links the found package's include dir — it
assumes `/usr/include` from the distro package; see spike notes):

```sh
CMAKE_PREFIX_PATH=$HOME/Documents/zk-data/direct-model-edit-rust/spikes/vendor-prefix \
CPATH=$HOME/Documents/zk-data/direct-model-edit-rust/spikes/vendor-prefix/include \
cargo build -p dme-harness --release --features vulkan
```

## Facts baked in (spike verdicts, 2026-11-06)

- **Spike 1 (llama-cpp-4 v0.11.0, PASS)**: qwen35 GGUF loads on the AMD 7700S
  via Vulkan at ~64 tok/s (Q8_0, decode); the embedded chat template renders
  both modes via `enable_thinking` (A = `assistant\n<tag>\n`, B =
  `assistant\n<tag>\n\n</tag>\n\n`); `TensorTransactions` capture `l_out-N`
  residual streams on the GDN hybrid; MLP graph node = `ffn_out-N` [1024, T].
  Constraints: layer 23 computes in a 1-row logits split (not
  batch-mappable — capture 0..=22); one context at a time; a selector's
  `maximum_rows` must cover the decode batch (the API refuses loudly).
- **Spike 2 (GGUF edit fidelity, PASS)**: the pure-Rust Q8_0 codec is
  bit-compatible with ggml (no-edit requant floor exactly 0.0). Edit
  round-trip on `blk.15.ffn_down.weight`: Q8_0 ~1.5e-3 rel; F16 2.1e-4;
  BF16 1.5e-3. The anchor surface (131,072 = 2 layers x 64 channels x 1024
  weights) is a *strided* subtraction: the 1024-dim direction is subtracted
  from 64 input channels (torch columns) of `blk.{L}.ffn_down.weight`.
  Patched files round-trip byte-identically.

## Deviations from the archived stage-1 receipts (declared)

- The instrument runs GGUF quants (Q8_0 / BF16) rather than HF bf16
  weights; every receipt carries `dtype` and `gguf_sha256`.
- Presence penalty uses llama.cpp's native penalties sampler with
  `penalty_last_n = n_ctx` (whole window) — the card's API semantics as
  approximated by llama.cpp; the archived rows used a
  transformers-side processor. Receipts carry `penalty_last_n`.
- Problem order: CPython `random.Random(20261106).shuffle` over the 7,473
  GSM8K train rows, reproduced by `mt.rs` and pinned against the archived
  `problem_id` sequences by `tests/mt_order.rs` (skip-if-absent).

## Data and model locations (this machine)

- Practice set: `~/Documents/zk-data/direct-model-edit-rust/data/gsm8k-train.jsonl`
- Configs: `~/Documents/zk-data/direct-model-edit-archive/configs/config-{A,B,S}.json`
- Models: `~/Documents/zk-data/direct-model-edit-rust/models/` and
  `~/Documents/zk-data/direct-model-edit-rust/vendor/gguf/` (the
  parallel-session substrate cache holds target BF16/Q8_0 and sibling BF16).

## Example

```sh
cargo run -p dme-harness --release --features vulkan -- probe \
  --gguf ~/Documents/zk-data/direct-model-edit-rust/vendor/gguf/Qwen3.5-0.8B-Q8_0.gguf \
  --config ~/Documents/zk-data/direct-model-edit-archive/configs/config-B.json \
  --problems ~/Documents/zk-data/direct-model-edit-rust/data/gsm8k-train.jsonl \
  --out /tmp/probe-B-smoke.jsonl --count 4 --cap 4096
```
