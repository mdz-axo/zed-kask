# dme-harness — the Rust direct-model-edit instrument

Operator ruling 2026-11-06: the Python replication workspace is retired and
archived; the live-replication harness is Rust + bash. This crate is that
instrument. The anchor case (MIT CSAIL "Sparks of Weight-Space Programming":
a direction per layer, applied to 64 output columns per layer of Qwen3.5-0.8B,
131,072 weights addressed ≈ 0.017% of the language model) is replicated with our
own measured receipts.

## Modes

| mode | what it does |
|---|---|
| `probe` | measurement leg: N GSM8K-train problems under a card-anchored config, against a llama.cpp server you run (bound to the GGUF by path; per-row sampling-parity verification), emitted as archived-schema JSONL rows + stage summary |
| `contrast` | the contrast-u leg (pure math, engine-free): a failing/progressing capture pair -> per-layer `u-{L}.f32`, the swiglu column ranking `ranked-{L}.txt`, and the S-mass margin receipt |
| `convert` | embedding-to-edit bridge (pure math, engine-free): embedder vectors + capture manifests -> `u.f32` / `k.f32` / `covdiag.f32` + a sha256-bound manifest + the edit spec; the explicit column-list trigger (corrective phase S2) round-trips a ranked column list into the sha-bound `columns.txt` + the `columns` spec instead; the manifest states the DME-2 projection (`projected_weights_touched`, `projected_edited_share` from the config's `total_weights`); the pre-emit lisp gate (`gates/pre-emit.lisp`) runs over the manifest at decision time |
| `edit` | direction-subtraction edit on a GGUF copy through the GGUF writer (Q8_0 minimal-collateral codec; F16/BF16 direct half arithmetic) + edit receipt |
| `gguf-info` | GGUF metadata/tensor dump + sha256 |
| `summary` | recompute a stage summary from a JSONL |

The DME gates are **not** in this crate: DME-0 (config), DME-1 (route),
DME-2 (budget), DME-3 (attribution), DME-4 (convergence) run in `lisp_eval`
at decision time over these receipts — the instrument measures, the gates
decide. One exception, from the corrective phase: DME-2 (the 0.1% surface
cap) is also **mechanical** in the `edit` leg — `edit::run` refuses any emit
whose projected surface exceeds 0.1% of the parsed GGUF's real param_count
(belt and braces with the lisp gate over the manifest's projection).

The edit trigger has three spellings: `channels` (the evenly-spread
indicator special case), `dense` (the per-column weight file k, optional
covdiag whitening), and `columns` (the sha-bound explicit column list —
k = indicator(S), the paper-faithful mechanism).

## The gates

`gates/` holds what decides over this instrument's files. The instrument
measures; it decides nothing.

| file | what it is |
|---|---|
| `pre-emit.lisp` | the gate over a `convert` manifest, run before an edit is emitted |
| `dme-measure.jq` | reads probe rows, `summary` output and edit receipts and prints the env for one skill gate: class counts, the table of two runs paired by problem id and seed, the recorded facts that differ between runs, the receipt chain. No thresholds, no verdicts |
| `dme-0.lisp`, `dme-2b.lisp`, `dme-2c.lisp`, `dme-2d.lisp`, `dme-3.lisp` | the direct-model-edit skill's measured gates; each is one complete `lisp_eval` form over the env `dme-measure.jq` prints |
| `dme-gate-cases.gen.jq`, `dme-gate-fixture.json` | the gates' declared cases: the generator builds each env with `dme-measure.jq` itself; the fixture holds verbatim excerpts of archived harness output |

`cargo test -p hkask-lisp --test dme_gates` builds every case with `jq` (required)
and evaluates it through the
interpreter `lisp_eval` uses. The skill
(`.agents/skills/direct-model-edit/SKILL.md`) gives the `jq` command for each
gate.

## The routes

The harness links no inference engine and launches no server.

- **Probe** talks to a llama.cpp server you start yourself. Its slot
  context must hold the generation cap plus 4,096 prompt tokens, and the
  server must have loaded the same file you pass as `--gguf` — the probe
  refuses otherwise, because the rows carry that file's sha256:
  ```
  llama-server -m /path/to/model.gguf -c 8192 -np 1 --port 8080
  dme-harness probe --gguf /path/to/model.gguf --endpoint http://127.0.0.1:8080 \
    --config config-B.json --problems gsm8k-train.jsonl --out rows.jsonl
  ```
  The probe uses llama-server's native `/props`, `/apply-template` and
  `/completion` routes; Ollama does not serve them. If the server was
  started with an API key, export it as `LLAMA_API_KEY`.
- **Edit** and **gguf-info** use the crate's own GGUF reader and writer
  (`src/gguf.rs`, `src/gguf_writer.rs`; single-file GGUFs).
- **Contrast** and **convert** read activation-capture sets
  (`manifest.jsonl` plus per-site `.f32` files). This crate no longer
  produces captures: the in-process capture and embedding legs were
  removed with the vendored llama.cpp binding (2026-11-11), so new
  capture sets must come from another tool.

## Build

`cargo build -p dme-harness --release`

The live GGUF gates run against real artifacts when
`DME_HARNESS_ARTIFACT_DIR` names the directory holding
`Qwen3.5-0.8B-BF16.gguf` and `Qwen3.5-0.8B-Q8_0.gguf`, and skip otherwise:
`DME_HARNESS_ARTIFACT_DIR=/path/to/dir cargo test -p dme-harness --test live_gguf_writer_gates -- --nocapture`

## The receipts' contract

Rows follow the archived instrument's JSONL schema verbatim (all archived
keys, none renamed or dropped) plus additive instrument keys only:
`harness`, `backend`, `gguf_sha256`, `penalty_last_n`, `n_ctx`,
`engine_version` (the server's `build_info`), `server_generation_settings`
(the sampling-parity receipt — every knob verified against the server's
echo, per row). The archived `device` and `threads` keys read `"server"`
and `0`: placement belongs to the server you started, and the harness
cannot observe it.
