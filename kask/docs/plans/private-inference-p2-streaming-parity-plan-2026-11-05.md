# Private-inference P2 — the moe-stream-partition streaming experiment (design + record)

**The approved shape** (local-leg plan §12 item 2, APPROVED 2026-11-05, operator-called
2026-11-05): the ServeurpersoCom `moe-stream-partition` fork, tested with this exact
model, behind the same wrapper, with the parity harness and its measured band.
Streaming stays **UNROUTABLE in v1 routing** until the band gates it — the
public-retraction lesson: streamed-vs-resident output diverges on long generations,
semantically close, not identical, so the band, and only the band, earns routability.
The parity-instrument follow-up (the fresh-slot protocol) **precedes** any band
adjudication. The measured input stands as recorded
(`kask/docs/research/private-inference-unsloth-mtp-measurement-2026-11-05.md`).

## 1. Direction (kata step 1)

The MTP record adjudicated a 16.52% P2 median gain with parity UNRESOLVED — the
identity instrument was confounded (D-G: within-leg temp-0 determinism fails at
reduced ctx because the server's slot-KV / preserved-reasoning state evolves across
requests). P2 asks the streaming question on top: does the three-tier streaming
engine (GPU expert cache + pinned-RAM mirror + SSD pread) serve this exact model
within a **measurable parity band** of the resident leg — with the instrument fixed
first. Routability is never assumed; it is earned by the band.

## 2. Current condition (measured 2026-11-05, this session)

- **The fork is pinned.** `engines/serveurperso-moe-stream-partition` cloned at
  branch tip `7174388895e8471a8bf2869e34d6a61e33f4b84c`
  (`BUILD_PIN.txt`, the qwen4exp-mtp precedent; `git ls-remote` and the clone's
  `rev-parse HEAD` agree — the exact-commit discipline, D-S2).
- **The fork's streaming vocabulary (in-source, pending live --help verification):**
  `--moe-stream` (enable), `--moe-stream-cache <NG|Ns>` (expert cache: GiB budget or
  exact slots-per-layer with an `s` suffix), `--moe-stream-ram <N>` (pinned host
  mirror budget in GiB; misses cross PCIe instead of reading the model file) —
  each *implies* `--moe-stream`. The engine emits moe-stream stats
  (`llama_moe_stream_print_stats`).
- **The fresh-slot flags exist on the fork's server:**
  `--reasoning-preserve / --no-reasoning-preserve` (the preserved-reasoning state is
  half of D-G's confound); the slot machinery (`server-context.cpp` reset paths) is
  the other half.
- **The wrapper's v1 stance:** `compile_placement`'s streaming slot fields stay
  `None` — "Streaming-mode launches are unroutable in v1"
  (`kask/crates/hkask-private-inference/src/local_leg/placement.rs` — the plan
  function's doc + the env construction); the constraint vocabulary already carries
  the streaming class (`expert_slot_floor`, `mutually_exclusive` `--mlock` vs
  `--moe-stream`, the slot fixtures).
- **The manifest surface family** (the wrapper's L1): registry TOMLs with
  exact-commit `[version_pin]`, probe paths, --help-verified flag spellings,
  engine-independent constraints + fixtures (llama-cpp-mainline.toml,
  llama-cpp-unsloth-prebuilt.toml).

## 3. Target condition (kata step 3)

The experiment answers three questions in order, each gated on the previous:

1. **The instrument gate.** Under the fresh-slot protocol (single slot,
   reasoning-preserve off, no cross-request slot reuse), within-leg temperature-0
   determinism HOLDS byte-identically at the experiment ctx — the D-G confound is
   removed before any identity claim.
2. **The parity band.** Streamed-vs-resident completions compared on the frozen
   suite plus a long-generation leg (the divergence class shows on long
   generations): byte identity, or the measured divergence envelope
   (first-divergence depth, divergence rate) — the band, recorded either way.
3. **The speed question.** Paired resident-vs-streaming decode (the P0 discipline,
   same session, discarded warmup, the consistency anchor: the resident leg's decode
   must land in the P0 13.0–15.0 tok/s class or the environment drifted and
   adjudication defers).

Streaming routability is NOT part of this engagement's deliverable: the band is the
input the operator's routing decision reads.

## 4. The fresh-slot protocol (the instrument, pre-declared)

The D-G evidence: within-leg reps diverge at exactly 29 common-prefix characters at
reduced ctx while holding byte-identical at ctx 65536 — the server's slot state
(KV reuse, preserved reasoning) evolves across requests. The protocol removes the
evolution:

- **Single slot**: `--parallel 1` (one serving slot — no slot rotation across
  requests; spelling verified at --help).
- **Reasoning preserve off**: `--no-reasoning-preserve` (no reasoning trace carried
  across requests).
- **No cross-request KV reuse**: the slot's prompt-cache reuse disabled (the exact
  spelling — `--cache-reuse` / its negation — verified at --help; if the fork has no
  such knob, the single-slot reset path is the mechanism and the protocol says so).
- **The instrument gate** (falsifiable): ≥3 reps per class, byte-identical
  completions within a leg, at the experiment ctx. If any rep diverges, the
  instrument is still confounded and NO cross-leg identity claim is made — the
  honest recording discipline of the MTP record (instrument-confounded, not
  parity-miss) applies.

## 5. The experiment legs (hand-launched, the P0 discipline)

Both engines launch by hand per leg (the canonical serving leg is the placement
compiler's; experiment legs are hand-launched and measured by the harness):

- **Resident leg (the control):** the mainline HIP engine
  (`engine/llama.cpp/build-hip`), `-ngl 999 --cpu-moe -fa on --kv-cache-type q8_0
  -c 32768` (the c′ precedent ctx — the MTP pair's clean-leg ctx), the frozen
  suite.
- **Streaming leg (the treatment):** the fork engine (pinned build), the same
  placement minus `--cpu-moe`, plus the three-tier streaming set:
  `--moe-stream --moe-stream-cache <budget> --moe-stream-ram <budget>` — the
  budgets sized from the machine's measured envelope (8.57 GB VRAM minus dense+KV+
  compute; 96 GB RAM minus the 72 GB model budget), recorded with the leg.
- **The long-generation leg:** the P2 prompt class at `max_tokens 1024` (the
  divergence class the public retraction observed on long generations) — the band's
  primary evidence.
- **The consistency anchor:** the resident leg's decode must land in the P0
  13.0–15.0 tok/s class (else environment-drifted; defer, never re-band).

## 6. The parity harness + the band arithmetic

The landed pair (named by `631d4468b2`): `kask/scripts/p2-parity-longleg.sh`
(the p0-bench-llama.sh shape: endpoint-driven against a hand-launched server,
records raw bodies/logs/timings per leg; the 1024-token long leg — three reps
plus a discarded warmup) and `kask/scripts/p2-parity-compare.sh` (the
comparison step over the saved bodies — the named-field discipline (jq), both
generated fields (`content` AND `reasoning_content`, the landed verdict's
thinking-model form), the adjudication arithmetic through `lisp_eval`
receipts at record time (the MTP record's frozen-arithmetic pattern)). The
frozen prompt and SYS line both extract from the ONE home
(p0-bench-llama.sh — `extract-frozen-p2-prompt.sh` + the longleg's sed; never
a second copy to drift). The band records: per class — byte-identical
(yes/no), first-divergence depth (bytes, post-decode), divergence rate
(divergent positions / completion tokens — computed by the receipts from the
recorded columns), and the long-generation leg's envelope. The band is the
measured record; the adjudication vocabulary (pre-declared):

- **Parity holds** — byte-identical completions on every class AND the long leg,
  instrument gate green.
- **Bounded divergence** — the band measured (depth + rate), instrument gate green;
  routability is then an operator decision reading the band.
- **Instrument-confounded** — the gate fails; recorded honestly, no band claim
  (the D-G discipline).

## 7. The v1-unroutable pin (the engagement's hard boundary)

Pinned in the crate's test family: the production (shipped) surface
(`llama-cpp-mainline.toml`) spells NO streaming vocabulary — a streaming launch
cannot be emitted by v1 routing; `compile_placement`'s slot fields stay `None`.
The fork's surface exists as an experiment manifest (decoded, fixture-gated,
probe-pinned) and is never the shipped surface.

## 8. Routing plan (steps, regions, gates)

| # | Step | Region | Gate |
|---|---|---|---|
| 1 | Acquire + pin + HIP build the fork | D | `PIN_MATCHES` (done, 2026-11-05); `BUILD_HIP_DONE` + the `--version` line |
| 2 | The manifest surface: `llama-cpp-serveurperso-moe-stream.toml`, spellings verified live against the built binary's `--help` | SD | decode + validate + every-fixture-gates (the crate's test family); `--version` commit == the pin |
| 3 | The fresh-slot instrument run (the gate before any identity claim) | D | ≥3 byte-identical reps per class at the experiment ctx |
| 4 | The paired legs + the long-generation leg | Sem-D | the consistency anchor holds; paired bodies + timings saved |
| 5 | The band + the lisp adjudication receipts | D | the vocabulary of §6; recorded either way |
| 6 | The v1-unroutable pin | SD | the crate's suite green |
| 7 | The record (§9) + the wrapper-plan §11 P2 note | Sem-P | this document |

## 9. Implementation record

**Attribution:** the code slices (the surface TOML, the three scripts, the
placement/manifest pins, this plan) landed in the concurrent stream's commit
`631d4468b2` ("Add the P2 streaming experiment surface and parity harness",
pushed to origin) alongside that stream's own media files — the shared-tree
sweep the `.rules` records; the work is intact under a faithful message. The
compare-script's instrument fix (the glob resolution + the reasoning-field +
the non-empty refusal) and this record land after it.

- **Step 1 (acquisition + build, 2026-11-05): DONE.** Cloned at tip
  `7174388895e8471a8bf2869e34d6a61e33f4b84c` (branch `moe-stream-partition`,
  `ServeurpersoCom/llama.cpp`), `BUILD_PIN.txt` written, `rev-parse HEAD` ==
  the ls-remote tip — **PIN_MATCHES**. HIP build completed (`BUILD_HIP_DONE`,
  the build-hip.sh recipe, ROCm 7.2); the binary's own `--version` line:
  `0.3.0-dev (build 1, commit 7174388)` — the fork branches from an OLDER
  upstream than mainline (0.3.0-dev vs 0.9.0-dev), so every spelling was
  re-verified live against this binary's `--help`.
- **Step 2 (the manifest surface): DONE.**
  `llama-cpp-serveurperso-moe-stream.toml` decodes, validates and
  every-fixture-gates (the crate's 52-test suite green). The fixture gate
  caught one real mismatch on landing (the mlock exclusivity fixture also
  tripping the RAM-headroom guard — mmap=false counts weights resident);
  fixed in place. The streaming vocabulary cannot ride `[flags]`
  (FlagVocabulary has no streaming intent — by design) — it lives in the
  provenance header; the legs pass the streaming flags by hand (the P0
  discipline). The engine's own abort validated the constraint model
  directly: `multi-pass expert GEMMs need at least 3*n_expert_used dynamic
  slots` — the same arithmetic as the surface's `expert_slot_floor`
  (floor_mult 3 × n_expert_used 10 = 30).
- **Step 3 (the fresh-slot instrument): FAILED at ctx 16384 — the
  instrument-confounded verdict.** The c′ deviation came first: the slot
  floor needs ~2.7 GiB of VRAM for the expert cache, and at ctx 32768 the
  compute-pool alloc OOM'd (the D-B class: `ROCm error: out of memory` at
  `ggml_cuda_pool_leg::alloc`) — the matched pair ran at **ctx 16384**
  (recorded, like D-C). Under the fresh-slot protocol (`--parallel 1
  --no-reasoning-preserve --no-cache-prompt`, spellings verified on BOTH
  engines' `--help`), within-leg temperature-0 determinism STILL fails on
  the short classes for BOTH engines (resident P1 reasoning 631/647/649
  chars, first divergence at byte ~310-316; streaming P1 652/659, byte
  316) — the confound persists through slot-state resets, so the mechanism
  is engine-compute-level at this quantization/ctx, a refinement of D-G
  (the slot-state hypothesis alone was insufficient). The resident's long
  class WAS byte-stable across its 3 reps (4350 chars × 3, byte-identical)
  while the streaming engine diverged within-leg there too. Per the
  pre-declared protocol the gate precedes the band: **no cross-leg
  identity claim is made; the band is UNMEASURED.**
- **Step 4 (the paired legs): DONE, anchor holds.** Both legs at ctx 16384,
  the frozen p0 suite + the long leg; artifacts under
  `measurements/p2-parity-2026-11-05/{resident,streaming}/`. The
  consistency anchor holds: the resident's engine-reported decode is
  **13.72 tok/s** — inside the P0 13.0-15.0 class (the environment did not
  drift; the configuration shift — single-slot, no-cache-prompt, ctx
  16384 — did not move the class).
- **Step 5 (the receipts): SPEED MISS + INSTRUMENT-CONFOUNDED.** The frozen
  lisp receipts (sign: negative = streaming slower):
  resident P2 median **24.83 s** vs streaming **56.53 s** → wall change
  **−127.7%** (2.28× slower); P3 −166.7%; the long leg warm 137–145 s vs
  84–90 s (−70.6% at the medians; the streaming r1 paid 348 s — the
  cold-cache SSD loads); engine-reported decode **9.02 vs 13.72 tok/s**
  (1.52× slower, −34%); `anchor_resident_decode_in_p0_13_15_class = true`.
  The M4 Max 36 GB datapoint (26 tok/s via streaming) does not transfer
  to the 8 GB card: the 30-slot cache holds too small an expert fraction
  and the misses cross PCIe/SSD.
- **Step 6 (the v1-unroutable pin): green.**
  `streaming_mode_stays_unroutable_in_v1` (placement) + the surface
  decode/fixture pins (manifest) — the production surface stays the
  mainline pin; the fork surface is an experiment manifest.

**The engagement's adjudication (the frozen vocabulary):** streaming does
NOT earn routability on this machine — three independent bars: (1) the
parity band unmeasured (instrument-confounded, the honest recording over a
forced claim); (2) the speed a MISS (slower, not a tie); (3) the streaming
engine's own within-request nondeterminism at the long class (it diverges
against itself at temperature 0 — a routable engine must at least be
self-consistent). Streaming stays **UNROUTABLE in v1**; the fork surface
stays pinned as the experiment record; a future streaming revisit needs
VRAM for a meaningful expert cache (the M4 Max class) or a parity instrument
that holds at this configuration.
