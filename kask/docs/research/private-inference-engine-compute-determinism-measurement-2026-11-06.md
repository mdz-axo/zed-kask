# Private-inference local leg — engine-compute determinism, gate-attempt measurement record (2026-11-06)

The engine-compute determinism study executed under its pre-registered design
(`private-inference-engine-compute-determinism-design-2026-11-06.md`) and the
execution prompt
(`kask/docs/plans/private-inference-determinism-study-execution-prompt-2026-11-06.md`,
commit `98513d9208` — the spend authorization). **Final outcome in one line
(after the §10 re-run): the study's premise is REFUTED by the gate's own
data — temperature-0 completions on this engine are fully deterministic
GIVEN THE REQUEST SEQUENCE (20 of 20 generated fields byte-identical across
two independent sessions, different processes, hours apart, at decode
speeds from 6.6 to 13.6 tok/s); the "nondeterminism" that confounded the
MTP and P2 records and closed the streaming bar was never randomness — it
is a deterministic, position-dependent server-state trajectory, and the
P2-B asymmetry (shorts flip, longs stable) is a position artifact (shorts
ran early-session, longs ran late-session at the settled attractor). The
gate's frozen acceptance still fails (the long class at early-session
positions is not byte-stable) → the confirmatory layer (O1–O4) is a
`measurement_void`, no arm ran, and the design's §2 rule sends the
observation back for new rivals — the re-ground proposal is §10.5.**
Spend: 11 + 11 requests across the two gate attempts; the arms unspent.

## 1. The frozen pre-registration this run was bound to

- **The gate (design §3):** the P1 class + the long class (P2 bytes at cap
  1024), 5 reps each, ctx 16384, temperature 0, the full fresh-slot protocol,
  on the hardened P2 parity harness discipline. Acceptance: the P2-B
  asymmetry REPRODUCES (short-class flips; long-class byte-stable) — else the
  design halts, the confirmatory layer is a `measurement_void`, and the
  operator decides whether to re-ground. Budget ≤10 measured requests.
- **The consistency anchor (inherited from all three prior records):** the
  resident's engine-reported decode must land in the P0 13.0–15.0 tok/s
  class — else environment-drifted and adjudication deferred, never
  re-banded. The anchor is an executable check on the environment claim,
  not a courtesy reading.
- **The arms (design §5, never reached):** O1 cap sweep {128,256,512,1024}×5
  (H2); O2 prompt-padding alignment mod n_ubatch=512 ×3 (H1); O3 ctx sweep
  {16384, 32768, 65536} ×3 + 2 relaunches (H1); O4 rep-order + one restart
  arm (H4). Fixed-N, no interim peeking.

## 2. Pre-flight (verified, no engine requests)

- **Engine pin (D-S2, live):** `build-hip/bin/llama-server --version` →
  `0.9.0-dev (build 1, commit 9e2a04a)` — exactly the mainline manifest's
  `[version_pin]` (`kask/registry/private-inference/llama-cpp-mainline.toml`).
  Fixture gate: `cargo test -p hkask-private-inference` → **54 passed, 0
  failed** (live/sysfs/gguf tests ignored by design).
- **Flag spellings verified live against the binary's `--help`** (the P2
  discipline): `-ctk/-ctv` (the split KV spellings), `-np/--parallel`,
  `--no-reasoning-preserve`, `--no-cache-prompt`, `-fa [on|off|auto]`,
  `-ot`, `-cmoe`, `-ngl`, `--host/--port`, `-a/--alias`, `-ub` (default 512,
  the O2 modulus).
- **The canonical root resolved live:**
  `~/.local/share/zed-kask/private-inference/engine/llama.cpp/build-hip/`
  (`engine_root()`; `HKASK_DATA_DIR` unset → the `~/.local/share/zed-kask`
  chain — the `~`-literal instrument rule applied, absence probed at both
  `~/Documents/zk-data` and `~/.local/share` before the XDG-default chain
  resolved it).
- **The study harness (committed with this record):**
  `kask/scripts/determinism-study-leg.sh` (the capture runner — frozen prompt
  bytes from the ONE home: `extract-frozen-p2-prompt.sh` + the SYS/P1 sed
  extraction; the exact p0 request shape; per-request req/resp/timing/srvlog
  evidence; the summary TSV) and `kask/scripts/determinism-study-compare.sh`
  (the identity observable — combined content+reasoning surface, the
  empty-side refusal, the zero-group guard, the rep-order envelope).
  **Instrument finding:** `p2-parity-compare.sh`'s `compare_bytes` carries a
  latent pipefail bug (`cmp -l | wc -l || echo 0` yields a multiline value
  on a differing pair) that never fired in P2 — the gate exited red before
  the band step ran, so the function was never executed on a real
  divergence (the zero-match instrument rule). The study's compare script
  carries the hardened form and HAS executed on real divergences (this
  gate's 8 divergent pairs).
- **The launch (P0 discipline, by hand):** the P2 resident reference
  configuration — `-ngl 999 --cpu-moe --override-tensor
  per_layer_token_embd=CPU -fa on --cache-type-k q8_0 --cache-type-v q8_0
  -c 16384 --parallel 1 --no-reasoning-preserve --no-cache-prompt --host
  127.0.0.1 --port 8010 --threads 12 --alias qwen3-8-flash-next` (pid
  2519377, log `server-gate-c16k.log`). Step-3 gates: health OK at ~29 s;
  `/v1/models` → `qwen3-8-flash-next` (n_ctx 16384, Q2_K, 79,569,728,000
  bytes); the engine banner confirms `n_slots = 1, n_ctx_slot = 16384` —
  the fresh-slot protocol engaged, never assumed.

## 3. Deviations, recorded with the data

- **D-S1 (the environment drift — the load-bearing deviation).** The gate
  ran into a concurrent GPU workload from another operator stream:
  `.venv-rocm/bin/python3 harness/dme.py probe --config A --problems 96
  --cap 4096 --device gpu --tag -96 --budget-secs 5400` (pid 2522801,
  started **18:28:00**, ~159% CPU, `--device gpu` — the direct-model-edit
  arc), plus an HF model download (`ornith-ai/Ornith-1.5-9B`, 18:27:56) and
  another stream's doc sweep. Load average at 18:40: 7.97/12.96/9.90. The
  server launched 18:25:24; the P1 phase straddled the onset (r1/r2 fast at
  13.64/12.92 tok/s, r3/r4 at **6.65/6.92** — half the anchor floor — at
  18:27:00–45), and the ENTIRE long-class phase ran under the dme.py probe
  (10.02, then 7.23/8.38/7.10/7.54). The P2 quiet-machine reference for the
  same long class: **11.70/13.11/13.41 tok/s** (the P2 resident leg's own
  eval reports). Evidence snapshot:
  `measurements/determinism-2026-11-06/environment-drift-evidence.txt`.
  Per the frozen anchor rule: **environment-drifted — adjudication deferred,
  never re-banded.**
- **D-S2 (the long-class gate shape, observed not adjudicated).** P2-B's
  long class was byte-stable across its 3 reps (4350 chars × 3, quiet
  machine). This gate's 5 reps: rep1 a `stop` form (917 ctok, content 545 —
  the only rep with non-empty content; combined 4227), rep2 a 4404-char
  `length` form, reps 3–5 byte-identical at **exactly the P2-B 4350-char
  form**. All five diverge from rep1 at byte 323 (the same depth); rep2
  diverges from the 4350 form at byte 667. NOT byte-stable across 5 — but
  with the anchor red, no attribution (premise vs environment vs
  order-effect) is admissible.

## 4. The gate measurement (the data)

P1@128, 5 reps (all `finish_reason "length"`, content 0 — the reasoning
surface is the combined surface):

| rep | reasoning chars | vs r1 | first-diff vs r1 (bytes) | decode tok/s | wall s |
|---|---|---|---|---|---|
| 1 | 631 | — | — | 13.64 | 12.796 |
| 2 | 647 | diff | 315 | 12.92 | 13.251 |
| 3 | 649 | diff | 309 | 6.65 | 22.507 |
| 4 | 621 | diff | 342 | 6.92 | 21.926 |
| 5 | 621 | diff | 342 | 13.03 | 13.191 |

P2@1024 (the long class), 5 reps:

| rep | finish | ctok | content | reasoning | combined | vs r1 | decode tok/s | wall s |
|---|---|---|---|---|---|---|---|---|
| 1 | stop | 917 | 545 | 3682 | 4227 | — | 10.02 | 98.948 |
| 2 | length | 1024 | 0 | 4404 | 4404 | diff@323 | 7.23 | 149.547 |
| 3 | length | 1024 | 0 | 4350 | 4350 | diff@323 | 8.38 | 130.635 |
| 4 | length | 1024 | 0 | 4350 | 4350 | diff@323 | 7.10 | 152.033 |
| 5 | length | 1024 | 0 | 4350 | 4350 | diff@323 | 7.54 | 143.234 |

Compare verdicts: `P1 cap128 NOT_ALL_SAME`, `P2 cap1024 NOT_ALL_SAME`
(`gate_identity.tsv`; NF=15 field-aware reads throughout — the tab-collapsed
display trap was caught live). Zero empty-side refusals; all 10 measured
requests parseable; the discarded warmup (P2@8) recorded at
`warmup/gatewarm_summary.tsv`.

## 5. Adjudication (the frozen vocabulary)

- **The gate: NOT ADJUDICABLE — environment-drifted.** The anchor fired
  mid-gate (D-S1): 2 of 10 measured reps in the 13.0–15.0 class, the rest at
  6.65–12.92; the long-class phase ran entirely under a competing GPU
  workload at 7.1–10.0 tok/s against the quiet-machine 11.7–13.4 reference.
  The strict gate criterion (long-class byte-stable) was not met in the raw
  data, but the anchor rule forbids reading that as a premise verdict —
  the cause cannot be attributed while the environment is invalid.
- **The confirmatory layer (O1–O4): `measurement_void`** — the design's §3
  halt clause. No arm ran; no factor was varied.
- **H1/H2/H3/H4: unadjudicated** — no arm data exists; each hypothesis
  stands exactly as the design froze it, never bent.
- **The deliverable layer: NOT REACHED.** Neither the determinism recipe
  nor the honest negative is claimed; the represent/execute statement below
  is limited to what this run actually measured.

## 6. lisp receipts (the frozen adjudication arithmetic)

- `p1_anchor_membership = [true, false, false, false, true]` —
  `long_anchor_membership = [false × 5]`; `anchor_in_class_count_of_10 = 2`
- `p1_halfspeed_ratio_fastest_vs_slowest = 2.05` (13.64/6.65)
- `p1_flip_band_min/max = 309/342` (vs the P2-B band ~310–316)
- `long_flip_depth_vs_r1_all_323 = true` (all four non-first reps diverge
  from rep1 at exactly byte 323)
- `long_identical_4350_count = 3`, `long_distinct_form_count = 3`,
  `empty_side_refusals = 0`
- `p1_wall_total_s = 83.671`, `long_wall_total_s = 674.397`,
  `measured_engine_time_s = 758.068`
- `spend_total_requests = 11` (10 measured + 1 discarded warmup);
  `spend_arms_requests = 0`

## 7. The represent/execute statement (this run's level)

The operator's goal: represent/reference invariants in prompts accurately
and consistently, and execute them accurately and consistently in processes.
This run's measured contribution:

1. **The anchor-as-executable-check worked exactly as designed — the
   gate's own represent/execute miniature.** The environment claim ("the
   machine is quiet enough for the 13.0–15.0 decode class") was REPRESENTED
   in the frozen records as a consistency anchor and EXECUTED by the gate
   reading the engine's own eval reports. That executable check is what
   stopped a false premise verdict at 11 requests instead of ~70 — without
   it, this gate would have been recorded as "premise unstable" and the
   four arms would have run against a contaminated environment. An
   invariant stated in configuration without an executable check is a
   hypothesis: this session is the pattern's cleanest demonstration yet.
2. **The short-class flip is not contention-caused.** The flip phenomenon
   reproduced under BOTH environments — quiet (P2-B: 631/647/649 chars,
   band ~310–316) and contended (this gate: 621–649 chars, band 309–342,
   reps both in and out of the anchor class flipping alike). Whatever
   flips the knife-edge tokens, it is not the concurrent load.
3. **What remains open: the long class's stability.** P2-B's 3-rep
   byte-stability (quiet) did not extend to 5 reps under contention — but
   the 4350-char byte-stable form EXISTS in this gate's data (3/5 reps,
   the exact P2-B form). Whether the long class is 5-rep stable on a
   quiet machine is precisely the re-run's first question; the premise's
   second half is neither confirmed nor refuted.

## 8. What stands for the re-run (the operator's decision points)

- **The environment condition:** the machine must be quiet — the dme.py
  probe's config A budget (5400 s from 18:28:00) ends ~19:58 local, and the
  DME arc may queue further configs; the HF download and other agent
  streams continue. The re-run's own pre-flight should verify: load
  average near idle AND the first measured reps decoding in the 13.0–15.0
  class before the gate is adjudicated.
- **The turnkey re-run:** relaunch the exact §2 command (same flags, log to
  a fresh `server-*.log`), then
  `determinism-study-leg.sh <endpoint> <pid> <log> <dir>/gate gate 5
  "P1:128,P2:1024"` + the compare — both scripts are committed with this
  record; the frozen prompt homes are unchanged.
- **Spend state:** 11 of ~70 authorized requests spent, 0 on arms. The
  authorization was composed for one gate + one confirmatory layer; a
  re-gate (11 requests) plus the arms (~59) exceeds it — the re-run needs
  the operator's explicit go (or a trimmed arm set the operator chooses).

## 9. Artifacts, provenance, and commit state

- Raw artifacts: `~/Documents/zk-data/private-inference/measurements/
  determinism-2026-11-06/` — `server.pid`, `server-gate-c16k.log`,
  `environment-drift-evidence.txt`, `warmup/`, `gate/` (per-request
  req/resp/timing/srvlog, `gate_summary.tsv`, `gate_identity.tsv`).
- The tree was clean at start at `98513d9208` (the execution prompt's own
  commit; 12 commits unpushed to origin/main at session start). This record
  and the two study scripts land in the commit named in the session report;
  the shared-tree discipline (staged-index check, pathspec commit, numstat
  verification) was applied.
- The server was stopped cleanly after the gate (port 8010 free, exit
  confirmed) — no servers left running.

## 10. The re-run (2026-11-06 ~19:40–20:00, operator-directed "proceed with both")

The operator's go covered the re-run's full frozen behavior — the gate, and
the arms only if the gate passed. The machine: quiet (load 1.33–1.78), AC
power, dGPU free (27 MB) — then, and only then, the same turnkey sequence:
launch (health ~1 s, warm page cache, banner `n_slots = 1, n_ctx_slot =
16384`), one discarded warmup (P2@8), the gate leg (P1:128 ×5, P2:1024 ×5).
Artifacts: `measurements/determinism-2026-11-06-rerun/`.

### 10.1 The anchor — thermal, recorded

Decode ran 8.38–9.57 tok/s across all ten reps — outside the 13.0–15.0
class a second time, on a quiet AC-powered machine. Cause: the dGPU's
thermal state after the evening's sustained GPU load — `rocm-smi` at the
halt: edge 63 °C, **junction 86 °C**, memory 80 °C. The anchor rule stands
(recorded, never re-banded); its concern — that environment drift could
manufacture false divergence — is answered empirically by §10.2's bytes:
drift moved the SPEED across 6.6→13.6 tok/s between reps and sessions and
did not move a single byte of the completions.

### 10.2 The cross-session byte check — the decisive observation

Every per-rep value in the re-run equals the first attempt's at the same
request position. Byte-level verification (jq extraction, `cmp` per field,
10 reps × 2 fields): **20 of 20 generated fields byte-identical across the
two independent sessions** — different processes, hours apart, decode
speeds spanning 6.6–13.6 tok/s. P1: 631/647/649/621/621 chars at the same
depths (315/309/342/342 vs r1; 315/309/309/539 adjacent). Long class:
4227-stop / 4404 / 4350×3 at the same depths (323 vs r1; 667 at the r2→r3
boundary). The re-run's r3–r5 = the P2-B byte-stable form (4350), and P2-B's
own short-class record (631/647/649) is the same trajectory's first three
positions — from a different day.

### 10.3 The finding — the premise is refuted

**Temperature-0 completions on this engine are fully deterministic GIVEN
the request sequence.** Same launch flags + same request order → same bytes,
every time, from every fresh process. The "knife-edge token flips" that
motivated this study, confounded the MTP record (D-G) and the P2 record
(P2-B), and kept the streaming bar closed are not nondeterminism: they are
a **deterministic, position-dependent server-state trajectory** — request 1
always yields the same completion, request 2 the next, and the sequence
converges to a stable attractor (the long class is byte-stable from its 3rd
request onward; P2-B's long leg, run LATE in its session after the full p0
suite, measured only the settled attractor and read it as a class
property). P2-B's short-vs-long "asymmetry" is a position artifact: the
shorts ran early-session (unsettled), the longs late-session (settled).

### 10.4 The frozen verdicts

- **The gate: FAILED its acceptance** — the P2-B asymmetry did not
  reproduce (the long class at early-session positions is not byte-stable
  across 5 reps). The design's §3 halt clause fires: the confirmatory layer
  (O1–O4) is a `measurement_void`; no arm ran; the anchor is recorded red
  (thermal) and was never re-banded.
- **The observation matches NO pre-registered rival** (design §2 — back for
  new rivals, never bent): **H1 refuted** (there is no run-to-run
  nondeterminism to attribute to compute-path selection); **H4's
  restart-reset prediction refuted** (a fresh process reproduces the
  trajectory identically — the "state" is a deterministic function of
  request history, not pool residue); H2/H3 unadjudicated (no arm data).
- **Spend:** attempt 1 + attempt 2 = 22 requests total; the arms' ~55
  requests unspent. Both attempts' engines stopped cleanly, ports free.

### 10.5 The re-ground proposal (the operator's decision)

The study's motivating question is answered by the gate's own data, without
the arms: the identity instrument was never measuring nondeterminism — it
was measuring position. The proposed re-ground:

1. **The recipe (the design's deliverable (a), proposed not claimed):**
   cross-engine identity comparisons are valid at MATCHED request positions
   — same warmup, same request order, compare rep-N to rep-N — or after
   the state settles (the long class reaches its byte-stable attractor by
   its 3rd request). Single-run byte-identity is real and reproducible
   under position control. The streaming-unroutable bar's stated precondition
   ("a parity instrument that holds at the operating configuration") is
   now constructible; whether to re-open streaming stays the operator's.
2. **The new rivals for a residual design** (what the old arms cannot
   test): the state-evolution MECHANISM (per-slot KV state? the engine's
   own slot LRU/similarity reuse paths?) and the minimal settle sequence
   (how many requests to the attractor, per class and cap) — each needs
   its own pre-registration before any spend.
3. **The prior records' verdicts stand as recorded** (instrument-confounded
   at their time) — but the MTP pair's parity question is now RE-OPENABLE
   under the position-matched protocol at the operator's direction.
