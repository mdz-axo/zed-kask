# Private-inference local leg — P2 streaming parity, measurement record (2026-11-05)

The operator-approved P2 engagement (local-leg plan §12 item 2; the engagement
plan: `kask/docs/plans/private-inference-p2-streaming-parity-plan-2026-11-05.md`).
The measured input stood as recorded
(`private-inference-unsloth-mtp-measurement-2026-11-05.md`); this record is the
streaming experiment's own pair. Artifacts:
`~/Documents/zk-data/private-inference/measurements/p2-parity-2026-11-05/`.

## 1. The frozen pre-registration (the engagement plan §3–§6)

- The fresh-slot instrument gate PRECEDES any identity claim (the MTP record's
  D-G follow-up): single slot (`--parallel 1`), `--no-reasoning-preserve`,
  `--no-cache-prompt` (spellings verified live on BOTH engines' `--help`),
  ≥3 byte-identical within-leg reps per class — else no cross-leg claim.
- The comparison observes the model's actual generated surface at these caps —
  `content` AND `reasoning_content` (the thinking model emits into
  reasoning first; finish_reason "length" with empty content is the normal
  shape) — and refuses a class whose combined surface is empty on either side.
- The pair: resident (mainline HIP, `--cpu-moe`) vs streaming (the pinned
  ServeurpersoCom `moe-stream-partition` fork, tip `7174388895e`, three-tier
  `--moe-stream --moe-stream-cache 30s --moe-stream-ram 40`), same frozen
  p0 suite + a 1024-token long leg (the divergence class), temperature 0,
  one discarded warmup each.
- The consistency anchor: the resident's engine-reported decode must land in
  the P0 13.0–15.0 tok/s class.

## 2. Deviations, recorded with the data

- **P2-A (the ctx reduction, D-C's class):** the slot floor is
  3×n_expert_used = 30 dynamic slots (~2.7 GiB VRAM — the engine's own abort
  names the same arithmetic as the surface's `expert_slot_floor`). At ctx 32768
  the compute-pool alloc OOM'd (`ROCm error: out of memory` at
  `ggml_cuda_pool_leg::alloc` — the D-B class); the matched pair ran at
  **ctx 16384**, both legs.
- **P2-B (the instrument, the D-G follow-up's outcome):** under the full
  fresh-slot protocol, within-leg temp-0 determinism STILL fails on the short
  classes for BOTH engines (resident P1 reasoning 631/647/649 chars, first
  divergence byte ~310–316; streaming P1 652/659, byte 316). The confound
  persists through slot-state resets — the mechanism is engine-compute-level
  at this quantization/ctx, refining D-G: the slot-state hypothesis alone is
  insufficient. The resident's long class was byte-stable across its 3 reps
  (4350 chars × 3); the streaming engine diverged within-leg there too.

## 3. The measured pair (ctx 16384, temperature 0, fresh-slot protocol)

| leg | P2 median s (r1/r2/r3) | P3 median s | long r1/r2/r3 s | decode tok/s (engine) | prompt eval tok/s |
|---|---|---|---|---|---|
| resident (mainline, cpu-moe) | **24.83** (25.34/24.43/24.83) | 27.07 | 90.1/85.0/84.0 | **13.72** (72.9 ms/tok) | 87.9 |
| streaming (fork, three-tier) | **56.53** (57.37/54.48/56.53) | 72.19 | 348.3/145.1/136.9 | **9.02** (110.9 ms/tok) | 20.2 |

The streaming long r1 (348 s) is the cold-cache leg — the first 1024-token
pass pays the SSD expert loads; the warm reps (137–145 s) are the steady state.

lisp receipts (the frozen adjudication arithmetic):
- P2 wall change **−127.7%** (streaming 2.28× slower); P3 −166.7%; long −70.6%
  at the medians
- decode ratio **1.52×** slower (13.72 → 9.02 tok/s)
- `anchor_resident_decode_in_p0_13_15_class = true` — the environment did not
  drift

## 4. Adjudication (the frozen vocabulary)

- **Parity: UNMEASURED — instrument-confounded.** The pre-declared gate
  failed (P2-B): within-leg determinism does not hold at ctx 16384 on either
  engine under the fresh-slot protocol, so no cross-leg identity claim is
  made and no band is recorded. The honest-recording discipline of the MTP
  record applies — calling this a parity result would misattribute
  engine-level noise to the streaming path.
- **Speed: MISS.** Slower than the resident leg on every class and in decode
  (1.52×). The M4 Max 36 GB datapoint (~26 tok/s via streaming) does not
  transfer to the 8 GB card: the floor-sized cache (30 slots) holds too small
  an expert fraction and the misses cross PCIe/SSD.
- **Routability: NOT EARNED — three independent bars.** The band unmeasured;
  the speed a miss; and the streaming engine's own within-request
  nondeterminism at the long class (it diverges against itself at temp 0 — a
  routable engine must at least be self-consistent). Streaming stays
  **unroutable in v1 routing** (pinned: `streaming_mode_stays_unroutable_in_v1`).

## 5. What stands for the next attempt

The pinned fork + surface manifest stay as the experiment record
(`llama-cpp-serveurperso-moe-stream.toml`, exact-commit pin `7174388`). A
future streaming revisit needs: VRAM for a meaningful expert cache (the M4
Max class where the datapoint was taken), and a parity instrument that holds
at the operating configuration — the fresh-slot protocol alone does not
restore within-leg determinism at reduced ctx on these engines. The
engine-compute determinism question (why 256-token completions flip
knife-edge tokens between reps while 1024-token completions on the same
engine can be byte-stable) is the named follow-up before any identity
instrument is trusted again.
