# llama.cpp consolidation plan — two surfaces, zero capability loss (2026-11-06)

Operator ruling 2026-11-06: no more than TWO llama.cpp surfaces — ollama's
bundled engine, and ONE owned by zed-kask. Every other surface goes. No
functionality or capability is killed: everything currently works must work
in the end state, through canonical routes and shared services.

REVISION 2 (synthesis pass, same day): the concurrent stream's spike
findings are folded in — the P5 "open capture issue" is RESOLVED (the
selector fix is known-good and documented below); the two DME harnesses
unify into ONE in-repo crate; P3's edit-leg refinements (minimal-collateral
codec, strided-column anchor surface, fidelity gates) and the P4 route
consumption correction (kask_bridge is gpui-coupled; the harness consumes
the endpoint semantics, not the crate) are integrated. The Requirements
(settled) section is unchanged and remains binding.

## End state

- **Surface 1 — ollama's bundled llama.cpp** (ollama 0.40.12): the
  convenience/fleet-convenience engine. Bonsai ternary verified served here
  (import success; direct-answer "4" receipt).
- **Surface 2 — the zed-kask engine**: the L1-pinned mainline (v0.9.0-dev,
  commit 9e2a04a — `registry/private-inference/llama-cpp-mainline.toml`'s
  own pin), built by a canonical in-repo recipe, artifacts at exactly one
  path (`~/Documents/zk-data/private-inference/engines/llama-cpp-mainline/
  build-hip/`), serving:
  the fleet (the L4 supervisor launches it per the L1 manifest — the
  startup probe's exact-commit expectation becomes satisfiable),
  the DME harness (post-migration, below),
  and the capture path (llama-cpp-4's `prebuilt` feature linking the
  canonical build's own libs — no second bundled engine; the sys crate's
  prebuilt_root/{lib,lib64,bin} resolution is the verified seam).
- **ONE DME harness** (the second settled consequence): the harness home is
  `kask/crates/dme-harness` — the operator's ruled location, the shared
  tree, and the only implementation carrying all four legs (probe, capture,
  edit, verify; the archived-schema receipt emitter and the MT19937 problem
  order oracle are already in it). The data-dir harness
  (`~/Documents/zk-data/direct-model-edit-rust/dme-harness/`) contributes
  its kept components (grade/gsm8k logic, the `--budget-secs` wall-clock
  bound, the problem-order oracle) and its 84-row transitional B record, then
  its tree deletes at the P4 gate. No duplicate harnesses survive.

## Capability map (all preserved, each with its verification gate)

| Capability | Today | End state | Gate |
|---|---|---|---|
| Fleet serving + drift absorption | unsloth b11160 used ad-hoc; pin absent | L4 supervisor launches the canonical build via the L1 manifest | the version probe prints the manifest's exact pin string (verified live: "0.9.0-dev (build 236, commit 9e2a04a)") |
| DME legs (B/S/A) | bespoke harness on b11160 (stopped at 84/96, the transitional record) | the ONE harness consumes the supervisor (LaunchPlan/endpoint) + the M3 `openai_compatible` route semantics. NOTE: kask_bridge is gpui-coupled — the standalone harness consumes the endpoint() request shape directly (the same semantics `kask_bridge/src/inference_providers.rs` routes for Zed); it does not link the bridge crate | sampling-parity leg: the route delivers temp/top_p/top_k/min_p/presence_penalty/repeat_penalty/seed/chat_template_kwargs exactly, verified per row from the server's `generation_settings` echo (penalty_last_n semantics pinned and echoed); a knob the route can't carry is a typed finding and an in-repo route fix, never a silent approximation |
| Capture (DME stage 3) | spike1's llama-cpp-4 BUNDLED build (a 3rd engine) | the same crate's `prebuilt` feature against the canonical libs. The spike's "l_out-0 shape mismatch" is RESOLVED, not open: selector `maximum_rows` must cover the decode batch; layers 0..=22 only (layer 23 computes in the 1-row logits split); ONE context at a time; selector lists name-sorted. Known-good pattern verified live (23 layer outputs captured, ffn_out-N [1024,T] enumerated, exit 0) | the known-good selector pattern fires on the canonical libs via prebuilt; a bindings/ABI mismatch against the pin's headers is a typed finding, fixed in-repo, never approximated |
| GGUF read + byte-exact edit (stages 4-5) | TWO hand-written parsers (dme gguf.rs; spike2) | ONE shared GGUF reader/writer — the L3 typed parse is the reader; the writer is spike2's proven byte-exact patch primitive plus the Q8_0 minimal-collateral requant codec (original block scale kept when the edit fits; unedited elements requant idempotently). The anchor's exact surface is built in: 131,072 = 2 layers × 64 channels × 1024 weights — the 1024-dim direction subtracted from 64 strided torch-columns of `blk.{15,19}.ffn_down.weight` | byte-identity on re-parse; param_count parity 752,393,024 (BF16 target); requant floor exactly 0.0 (codec bit-compatibility with ggml); a patched-GGUF generation smoke (load + short probe proves the edit is live); spike2's fidelity record (Q8_0 ~1.5e-3, F16 2.1e-4, BF16 1.5e-3 rel) stands as the BF16-edit justification |
| Bonsai ternary | ollama (verified) | ollama — unchanged | done (receipt on file) |
| MTP draft-head experiments (qwen4exp-mtp, 6fcaa16) | a live 2nd/3rd engine tree | capability preserved as an in-repo patch file (the tree's delta vs its BUILD_PIN base) — applied/rebased onto the pin when that thread needs it; the TREE deletes | the patch applies cleanly against its recorded base sha |
| MoE stream partition (serveurperso, 7174388) | same | same disposition | same |
| Model availability | scattered: workspace vendor/gguf, ollama blobs, HF cache, the spikes' models/ | one canonical models dir (zk-data/private-inference/models/) + a registry manifest; ollama imports reference it; the HF cache remains a download cache only | every GGUF file accounted (moved, not lost); no duplicate stores (the spikes' Ornith Q8_0/BF16 and Qwen Q8_0 duplicates reconcile here) |

## Work order

- **P0 (done):** proliferation frozen; the leg chain stopped at 84/96; the
  transitional B record stands (`logs/probe-B-full-rust.jsonl`, never
  resumed); nothing new builds or launches on non-canonical surfaces.
- **P1 — canonical engine adoption:** `kask/scripts/build-llamacpp-mainline.sh`
  (in-repo): checkout the pin → cmake HIP gfx1102 with the manifest-conformant
  flags (GGML_HIP=ON, AMDGPU_TARGETS=gfx1102, CMAKE_BUILD_TYPE=Release,
  CMAKE_HIP_PLATFORM=amd, GGML_HIP_NO_VMM=ON, GGML_HIP_GRAPHS=ON,
  GGML_HIP_MMQ_MFMA=ON, GGML_HIP_RCCL=OFF) → the L1 version probe. The
  existing build is the canonical tree's first build; the script makes it
  reproducible from the repo alone. GATE: the script reproduces the build
  from the repo alone; the probe prints the pinned commit; the startup probe
  accepts it (the probe parses the COMMIT — if any fixture compares the
  build number, the manifest's "build 1" example updates to the measured
  build per the drift-absorption discipline).
- **P2 — patch extraction, tree deletion:** extract both experimental trees'
  deltas as in-repo patches with their BUILD_PIN shas + provenance notes
  (kask/registry/private-inference/patches/); delete `engines/qwen4exp-mtp/`
  and `engines/serveurperso-moe-stream-partition/`. GATE: each patch applies
  cleanly against its recorded base.
- **P3 — shared GGUF component:** the reader (L3 typed parse) + the writer
  (spike2's primitive + the minimal-collateral requant + the strided-column
  edit addressing) become one component; both hand-rolled parsers delete
  after their logic ports. Both DME consumers + the L3 consume it. GATES:
  byte-identity on re-parse; param_count parity 752,393,024 on the BF16
  target; requant floor 0.0; the patched-GGUF generation smoke.
- **P4 — harness migration + unification:** the ONE harness at
  `kask/crates/dme-harness` keeps ONLY DME-specific logic (CPython-compatible
  problem selection, answer-class grading, the archived row schema, trap-config
  provenance, the DME gates, the edit/capture instrumentation calls) and
  consumes the private-inference stack: L1 manifest (flags), L3 placement
  (GGUF parse via the P3 component), L4 supervisor (launch/health/lifecycle),
  the M3 route semantics for completions (direct to endpoint() — see the
  capability map note). The data-dir harness's kept components migrate in
  (grade/gsm8k logic, `--budget-secs`, the oracle ids); its launcher/HTTP/
  flags/polling code never ports; the data-dir tree deletes at the gate.
  GATES: (a) sampling-parity (see the map); (b) the problem-order oracle —
  first ids remain [gsm8k-train-6819, gsm8k-train-3171, gsm8k-train-5037,
  gsm8k-train-6229, gsm8k-train-1026]; (c) receipt contract — archived row
  keys verbatim plus only additive instrument keys (harness, backend,
  gguf_sha256, penalty_last_n, n_ctx).
- **P5 — capture migration:** spike1's FIXED selector logic relinks via
  `prebuilt` (prebuilt_root = the canonical build dir); the l_out-0 note in
  the first draft was a stale intermediate log — the resolution is: selector
  maximum_rows ≥ decode batch; layers 0..=22; one context at a time;
  name-sorted selector lists; ffn_out-N/l_out-N are the capture sites.
  DELETE: the bundled build artifacts, `spikes/llamacpp4-src/`, spike1's
  cmake target cache. GATE: the known-good capture pattern (23 layers) fires
  on the canonical libs; any bindgen/ABI mismatch against the pin's headers
  is a typed finding fixed in-repo.
- **P6 — model consolidation:** the canonical models dir + registry
  manifest; the four GGUFs (Qwen BF16 + Q8_0, Ornith BF16, Bonsai) move;
  the spikes' `models/` dir merges (its Ornith Q8_0 and duplicate Qwen Q8_0
  reconcile against the registry manifest — no duplicate stores); ollama's
  bonsai-27b import re-checks. GATE: every GGUF accounted (moved, not lost).
- **P7 — the legs resume, clean:** B re-run (96, config B, cap 32768), S
  (Ornith-1.5-9B BF16, config S, 10 problems — the skill's non-looping
  sibling gate), A (96 × 32768, the trap baseline) — all through the
  canonical route, fresh tags (`-canonical`), the live DME-0/DME-1 verdicts
  per leg over its summary. One instrument for all stage-2 measurements.
- **P8 — surface audit:** enumerate every llama.cpp surface the project
  touches. Expect exactly two: ollama's bundled engine and the canonical
  build. Any other live dependency is a finding, not a footnote.

## Deletions (explicit)

`engines/qwen4exp-mtp/`, `engines/serveurperso-moe-stream-partition/`
(after P2), `spikes/llamacpp4-src/` + spike1's bundled build artifacts
(after P5), both hand GGUF parsers (after P3), the data-dir harness tree
(after the P4 gate), the bespoke launcher/HTTP/flag code paths (in P4).
Capability code ports first, then the old path deletes — never both live.

## Requirements (settled by the operator's ruling 2026-11-06 — not open questions)

The zed-kask private inference code IS the private inference engine: the
L1-pinned llama.cpp (v0.9.0-dev, commit 9e2a04a) is the ONE project
surface, and ALL project inference — fleet (L4 supervisor), the DME
harness, the capture path — runs on it. The unsloth prebuilt
(~/.unsloth/llama.cpp) is NOT a project surface and never was: it is
external Desktop tooling; no project process builds on it, and the DME
legs that ran on it are the transitional record, superseded by the
canonical re-runs. The pin is the manifest's (the drift-absorption layer
owns any future re-pin). The experimental trees go with their capability
preserved as in-repo patches at their BUILD_PIN shas. Receipt artifacts are
kept in the canonical models dir. Zero functionality is killed; every
capability's gate is named above.

Unchanged by this plan: the two review cards (C1-C5, DME-1S) await the
walk; the stage-1 archive records remain historical context per their note.

## Standing lesson (binds this plan's execution)

Curator memory, entity `stack-selection`, attribute
`installed-infrastructure-first` (three incidents recorded 2026-11-06,
pooled confidence ~0.78): consume the project's shared services and
canonical routes; never build parallel infrastructure, duplicate
transports, or ad-hoc drivers; probe what exists before building anything;
capability ports first, then the duplicate DELETES — no racing paths, no
shims. The harness unification in P4 is the direct remediation of this
lesson's third instance; a fourth instance is a reportable finding.
