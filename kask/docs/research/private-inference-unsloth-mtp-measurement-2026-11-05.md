# Private-inference local leg — Unsloth MTP transfer, measurement record (2026-11-05)

The T1–T7 arc of the Unsloth technique-transfer plan
(`kask/docs/plans/private-inference-unsloth-transfer-plan-2026-11-04.md`):
inventory (research run `1c20abbaea046125`) → adjudication (R2 GATE) →
pre-registered experiment (kata task `85bb1b10`) → implementation
(`6a1adb7699`) → invariant proofs (`332b6a538f`) → this paired measurement.

## 1. The frozen pre-registration (recorded before any run)

- **Primary endpoint:** P2 agent-turn (256 tokens, non-stream) wall seconds,
  median of 3 reps, **paired baseline-vs-MTP legs** through the byte-frozen
  `kask/scripts/p0-bench-llama.sh`, same session, temperature 0, one
  discarded warmup each.
- **Prediction:** paired P2 median reduction 10–37% (anchors: unsloth's
  1.15x low-bandwidth floor → ~10% wall; 1.3x → ~21%; 1.7x GPU claim →
  31–37%; the M4 Max 26 tok/s datapoint an upper context anchor only).
- **Noise band:** ±8% of the paired baseline median (from the P0 HIP rep
  half-spreads: decode 7.14%, agent-turn 7.77% — lisp receipts).
- **Adjudication:** >8% = gain; ≤8% = tie; slower = miss (revert);
  temperature-0 completion-token divergence = parity miss.
- **Consistency anchor:** the paired baseline's decode must land in the
  P0 record's 13.0–15.0 tok/s class, else environment-drifted and
  adjudication deferred — never re-banded.

## 2. Engines and provenance

| engine | pin | role | outcome |
|---|---|---|---|
| unsloth prebuilt | release `b11160-mix-a6922cc`, source commit `a3c12db9…` (unslothai/llama.cpp), asset sha256 `0bc43f5b…`, ROCM gfx110X | the pre-run substitution for the fork build | baseline drifted (5.34 tok/s — anchor fired); treatment crashed (ROCm OOM at first speculative inference) |
| qwen4exp/mtp fork | branch tip `6fcaa16f4b360649933a54d1f91ad40ed35c0e11` (danielhanchen/llama.cpp), built HIP gfx1102 on this machine (ROCm 7.2, `engines/qwen4exp-mtp/BUILD_PIN.txt`) | the original T3 engine plan, restored | the measured pair |

Draft module: `mtp-Qwen3.8-Flash-Next-shared-Q8_0.gguf`,
2,786,568,256 bytes, sha256
`5ff54097406a905cf3a724c709124ceb0e3e10235ee862298969e91c96fa96e6`
(matches the HF LFS oid), resolved through the Qwen brief's `[draft]`
spec at the shard dir's `MTP/` sibling.

## 3. Deviations, recorded with the data

- **D-A (prebuilt drift):** the prebuilt baseline (leg e) decoded at
  **5.34 tok/s** (engine eval report: 187.18 ms/token; P2 median
  48.37 s) against the 13.0–15.0 class — the anchor fired; adjudication
  deferred per protocol. The prebuilt treatment leg died at the first
  speculative inference (curl 52, empty reply; the draft had loaded).
- **D-B (ctx 65536 OOM):** the fork treatment (leg f2) crashed the same
  way; the engine's own crash handler names the cause —
  **`ROCm error: out of memory` at `ggml_cuda_pool_leg::alloc`** — the
  speculative-verify path needs GPU workspace the 8 GB card does not
  have at ctx 65536 (the card measured 0.9–1.2 GB free against
  unsloth's own "prepare 1–2 GB extra headroom" requirement, recorded
  at T1). Blocked, not a miss — recorded as such.
- **D-C (ctx reduction, the c′ precedent):** the spec pair ran at
  ctx 16384 (f3/e3) then the clean pair at **ctx 32768** (f4/e4) — the
  recorded flag deviation that frees the verify workspace. The frozen
  suite (584/913-token prompts, 256-token completions) fits either.
- **D-E (TSV token columns):** the fork engines' usage format misparses
  in the bench's `usage_of` (prompt+completion concatenated, completion
  empty) — token counts in this record come from the response JSONs via
  jq. A bench patch is a named follow-up, not applied mid-experiment.
- **D-F (TTFT):** the stream TTFB flushes before the first token on
  these engines (0.015 s on a 584-token prefill) — the TTFT candidate is
  invalid on them; TTFT unmeasured (the instrument header's named
  contingency).
- **D-G (the determinism confound):** within-leg temp-0 determinism
  **fails at ctx 16384/32768** (every baseline rep unique) while holding
  at ctx 65536 (byte-identical reps on both engines) — the server's
  slot-KV / preserved-reasoning state evolves across requests. Prefix
  evidence: the MTP leg diverges from the baseline at **exactly 29
  common-prefix characters — the same depth at which the baseline's own
  reps diverge from each other**. The cross-leg identity comparison is
  therefore instrument-confounded; divergence cannot be attributed to
  the draft.

## 4. The measured pair (ctx 32768, the fork engine, temperature 0)

| leg | P2 median s (r1/r2/r3) | P1 median s | P3 median s | decode tok/s (engine report) | VmRSS |
|---|---|---|---|---|---|
| e4 — baseline | **19.860** (20.11/19.54/19.86) | 9.843 | 19.531 | **13.00** (76.90 ms/tok) | 52.1 GB |
| f4 — +MTP draft | **16.580** (16.59/16.58/15.65) | 7.706 | 16.871 | **16.57** (60.34 ms/tok) | 55.3 GB |

Engagement evidence (the engine's own speculative telemetry, f4 P2 r3):
`draft acceptance = 0.66514 (145 accepted / 218 generated), mean len = 2.33`;
acceptance ranged 0.59–0.75 across requests. VRAM at 94% post-suite
(8.02 of 8.57 GB) — the workspace the ctx reduction bought.

lisp receipts (the frozen adjudication arithmetic):
- P2 median reduction **16.52%** — `gain_beyond_8pct_band = true`,
  `inside_prediction_band_10_37 = true`
- P1 reduction 21.71%; P3 reduction 13.62%
- decode speedup **1.275x** (13.00 → 16.57); P2 wall speedup 1.198x

## 5. Adjudication (the frozen vocabulary)

- **GAIN.** The pre-registered primary endpoint cleared its band: 16.52%
  P2 median reduction, beyond the pre-declared ±8% noise band, inside
  the pre-declared 10–37% prediction band, with the consistency anchor
  holding exactly (e4 decode 13.00 tok/s — the P0 floor). The gain
  replicates across prompt classes (P1 21.7%, P3 13.6%).
- **Parity: UNRESOLVED — not parity-miss.** The pre-registered
  identity gate could not be adjudicated on this machine: the
  instrument's temp-0 determinism premise fails at every ctx where the
  technique fits the card (D-G), and the prefix evidence (29 chars,
  identical within-leg and cross-leg) exonerates the draft as the
  divergence cause. Calling this a parity miss would misattribute the
  baseline's own noise to the technique. The engine's
  only-verified-tokens-kept claim remains unsloth-claimed, not
  machine-verified here.
- **Blocked paths, recorded not smoothed:** the prebuilt engine (drift +
  draft-path crash); MTP at ctx 65536 on this 8 GB card (the verify
  workspace does not fit — D-B). The production-integration constraint
  follows: **MTP on this card requires a reduced ctx** (or more VRAM
  than the 8 GB card has) — the wrapper's draft routing does not yet
  know the 1–2 GB workspace term (see follow-ups).

## 6. The grill record (T6 adversarial review)

- **Recall:** the implementation launches the technique, not its name —
  the module resolves fail-closed, the routing is arithmetic, the flags
  are surface-verified spellings, and the engine's acceptance telemetry
  proves the mechanism engaged (draft → verify → accept, 0.59–0.75).
- **Mechanism:** routing ⇒ guard is Lean-proven over stated assumptions
  (`kask/lean/draft_routing_mtp.lean`, `332b6a538f`); the refusals are
  typed (`DraftBriefMissing`, `DraftVocabularyMissing`,
  `DraftModuleMissing`); the emission is total.
- **Rationale:** the CPU-routed draft follows from the measured VRAM
  arithmetic (0.9–1.2 GB free vs a 2.79 GB module) — and the OOM finding
  refined it: the verify workspace needs GPU headroom independent of
  the draft's own placement, which the current arithmetic
  under-reserves (the named follow-up).
- **Edge cases:** OOM mid-session observed and typed (the ROCm backtrace);
  flag drift absorbed as surface data (each engine carries its verified
  spellings; mainline claims none); iGPU-vs-dGPU handled at the probe
  (largest card wins); warm-vs-cold controlled by the discarded warmup
  and the slot-cache D2 precedent; the streaming-retraction class is
  what the parity gate exists for — and its honest failure mode
  (instrument-confounded, prefix-evidenced) was recorded instead of
  forced.
- **Synthesis:** the technique transfers — GATE resolved to a measured,
  band-adjudicated gain with two production constraints surfaced by the
  measurement itself: the workspace headroom term (missing from the
  routing arithmetic) and the ctx policy (MTP needs the reduced ctx on
  this card). Neither blocks the finding; both gate the default-on
  routing decision.

## 7. Follow-ups (proposed, not done here)

1. **The workspace term:** the draft routing should reserve the
   spec-decode verify workspace (unsloth's 1–2 GB guidance) in the VRAM
   arithmetic — a model-brief/manifest field plus the guard extension,
   with the Lean proof extended over the reserved term.
2. **The parity instrument:** a fresh-slot protocol (single-slot /
   per-request state reset / `--no-reasoning-preserve`) before any
   future identity claim.
3. **Bench repairs (post-experiment):** the `usage_of` parse for these
   engines' usage format (D-E); the first-content-line TTFT timer
   (D-F).
4. **`.rules` proposals** (filed in the arc report, not edited inline):
   the shipped-lib CWD-relative-path trap; the cross-instrument
   comparison trap.

## 8. Post-experiment amendments (2026-11-06 — post-freeze; no re-run)

- **D-E corrected — the TSVs were right all along; `usage_of` needs no
  patch.** The recorded symptom ("prompt+completion concatenated,
  completion empty") was a tab-collapsed display reading of a correct
  TSV, not a parse defect: a field-aware sweep of every row of all six
  leg summaries (`awk -F'\t'`, 2026-11-06) reads NF=10 with the token
  fields correctly split (P1 `[123] [128]`, P2 `[584] [256]`, P3 `[913]
  [256]`; stream rows honestly blank — SSE bodies carry no usage), and
  the step-by-step `usage_of` trace over the e4/f4 response JSONs yields
  584/256. The record's jq-derived token counts agree with the TSV
  bytes. §7 follow-up 3's `usage_of` half is closed as no-defect; the
  display-reading lesson landed as the cross-instrument comparison rule
  (`.rules`, `## Other traps`, 2026-11-06). No number in §4 moves — the
  frozen measurements are unchanged by this correction.
- **D-F repaired in the harness — the stream TTFT is now the first
  non-empty `content` line's arrival.** The script header's pre-declared
  contingency fired on the first legs (TTFB flushes at 0.015 s on the
  584-token prefill), and the mid-experiment freeze ended with the P2
  close-out: `kask/scripts/p0-bench-llama.sh` now times the first content
  line (a rep with no content line within the cap leaves `ttft_s` blank
  — refuse the empty-side comparison), keeps TTFB as the flush
  observable (first SSE line arrival), and takes the stream total as
  wall-clock. The prompt suite and every non-stream column are
  unchanged; the deviation is recorded in the script header. TTFT itself
  remains unmeasured for the fork engines until a leg re-runs under the
  patched harness.
