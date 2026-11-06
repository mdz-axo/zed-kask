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

`kask/scripts/p2-parity-llama.sh` (the p0-bench-llama.sh shape: endpoint-driven,
records raw bodies/logs/timings per leg) plus a comparison step over the saved
bodies — the named-field discipline (jq), the adjudication arithmetic through
`lisp_eval` receipts (the MTP record's frozen-arithmetic pattern). The band
records: per class — byte-identical (yes/no), first-divergence character depth,
divergence token rate (divergent positions / completion tokens), and the
long-generation leg's envelope. The band is the measured record; the adjudication
vocabulary (pre-declared):

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

- **Step 1 (acquisition + build, 2026-11-05):** cloned at tip
  `7174388895e8471a8bf2869e34d6a61e33f4b84c` (branch `moe-stream-partition`,
  `ServeurpersoCom/llama.cpp`), `BUILD_PIN.txt` written, `rev-parse HEAD` == the
  ls-remote tip — **PIN_MATCHES**. HIP build launched (the build-hip.sh recipe:
  `GGML_HIP=ON, AMDGPU_TARGETS=gfx1102, ROCm 7.2`) — outcome recorded below when
  `BUILD_HIP_DONE`.
