# Private-inference local leg — MTP parity re-check under the position-matched protocol (pre-spend design, 2026-11-06)

`design-spec-v1`-shaped, authored BEFORE any spend; the proving run of the
position-matched identity protocol
(`private-inference-position-matched-identity-protocol-2026-11-06.md`) and
the gate on the MTP default-on routing decision. The question is the MTP
record's unresolved pair
(`private-inference-unsloth-mtp-measurement-2026-11-05.md` §5): the 16.52%
P2 speed gain is measured; the draft's byte-faithfulness was left
UNRESOLVED because the then-current instrument read the position
trajectory as nondeterminism (D-G). The position-matched protocol
re-opens the question cleanly.

## 1. The question

Does the qwen4exp draft-verified speculative path produce completions
byte-identical to the resident's at matched request positions?

## 2. The pair (the MTP record's clean-pair configuration, ctx 32768)

- **Resident baseline leg (e4 shape):** mainline `9e2a04a`, HIP build,
  `-ngl 999 --cpu-moe --override-tensor per_layer_token_embd=CPU -fa on
  --cache-type-k q8_0 --cache-type-v q8_0 -c 32768 --host 127.0.0.1
  --port 8010 --threads 12 --alias qwen3-8-flash-next`, fresh-slot
  protocol NOT required (position-matching replaces it — the protocol's
  §2 discipline; the state trajectory is part of the matched sequence).
- **MTP leg (f4 shape):** the qwen4exp fork `6fcaa16`
  (`engines/qwen4exp-mtp/build-hip/bin/llama-server`), the same placement,
  plus the draft module
  `models/qwen38-flash-next/UD-Q2_K_XL/MTP/mtp-Qwen3.8-Flash-Next-shared-Q8_0.gguf`
  (sha256
  `5ff54097406a905cf3a724c709124ceb0e3e10235ee862298969e91c96fa96e6`,
  the MTP record §2 pin) and the fork's spec-decode flags — EVERY spelling
  verified live on the fork's `--help` at launch (the fork branches from
  an older upstream; its vocabulary is not the mainline's).
- **Both legs run the SAME frozen sequence:** one `p0-bench-llama.sh` pass
  each (discarded P2 warmup, P1/P2/P3 × 3 non-stream + 1 stream, the
  byte-frozen suite) — positions match by construction. The comparison is
  engine-level: non-stream bodies only.

## 3. Predictions (frozen before any spend)

- **Draft-faithful:** every matched position byte-identical on the
  combined content+reasoning surface → **parity VERIFIED**; the MTP
  record's "only-verified-tokens-kept" claim machine-verified; the
  default-on routing question proceeds on verified ground.
- **Draft-perturbing:** any matched position diverges → **parity MISS**
  with the envelope reported (first-diff depth, differing bytes, lengths);
  the MTP story changes honestly — the draft perturbs output at matched
  positions, and the routing decision reads that.

## 4. The gates (frozen)

- **Thermal:** the legs run only when the dGPU junction reads ≤ 70 °C with
  no competing GPU process. Deferral evidence at design time (2026-11-06
  20:15): junction 100 °C, memory 104 °C, under the concurrent rebuild
  stream's GPU server — the legs are deferred until the card cools and the
  machine is otherwise idle.
- **The comparison:** `kask/scripts/position-matched-compare.sh` —
  rep-N vs rep-N per class, the combined surface, the empty-side refusal,
  the zero-group guard.
- **Environment recorded per leg:** engine-reported decode, load,
  `rocm-smi` junction — recorded as metadata per the protocol's §2.6, not
  adjudication refusals.

## 5. The spend bound

2 legs × 13 requests (the p0-bench suite: 1 warmup + 9 non-stream + 3
stream) = **26 engine requests, one session when the gate opens.** No
other spend rides this design.

## 6. Out of scope

Speed re-measurement (the 16.52% stands as measured); streaming (held per
the standing v1-unroutable pin; its precondition is now constructible but
the speed bar stands on this card); the state-evolution mechanism study.

## 7. Disposition (2026-11-07): superseded by the two-surface consolidation
ruling — VOID as written, zero spend

The operator's 2026-11-06 ruling (the consolidation plan v2: one served
surface = ollama; one instrument surface = the in-process llama-cpp-4
binding; every other llama.cpp tree goes) removed this design's treatment
engine: `engines/qwen4exp-mtp` was deleted, with the MTP capability
re-landing as `MtpSession` on the binding behind the plan's V3 gate. The
design is therefore VOID in the executor-unavailable class (§4's typed
vocabulary): the two-served-engine comparison it froze no longer has two
engines to compare. **No spend occurred** — 0 of the 26 authorized
requests; the deferred legs were never run.

The parity question survives in a cleaner form at the V3 gate: MtpSession's
speculative path vs the non-speculative path, compared IN-PROCESS on the
one instrument (the plan's own one-instrument-comparability principle),
under the position-matched protocol
(`private-inference-position-matched-identity-protocol-2026-11-06.md`) —
the state-trajectory finding applies to the binding's engine equally:
same warmup, same problem sequence, matched positions, or the baselines
drift with session position exactly as the served-engine studies did.

Close-out note: the fork deletions ran without the plan's own archive-
tarball-first gate (no archive found; the research record survives via
the repo docs — the MTP and P2 records carry every commit pin and
sha256, and both forks are re-clonable at their pinned commits).
