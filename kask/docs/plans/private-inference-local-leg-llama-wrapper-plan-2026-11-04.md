# Private-inference local leg — llama.cpp wrapper architecture plan (refined + critiqued)

**Status:** plan for the implementation engagement; extends the committed D-3 architecture
plan (M1 local-leg module). Does not reopen M3 (configured `openai_compatible` instance, no
new provider code), M6 (routing hook), or M7 (settings home). **Date:** 2026-11-04.

**Provenance.** Refined through the nine-skill battery (refactor-architecture,
pragmatic-cybernetics, pragmatic-semantics, grill-me, diagnose, hypothesis-framer,
kata-improvement, essentialist, region-routing), then critiqued and evolved; the deltas are
recorded in §10. Deterministic gates executed live at use time:
RR-1 routing-plan invariant `gate_passed=true, bad_steps=[]` (after reconstructing the
documented form in the proven `begin`+`define` shape — incident filed);
DIAG-1 hypothesis invariants `ok`; PC convergence run 1 `variety_deficit` (4 response
classes vs 5 disturbance classes with streaming routable) → run 2 `viable` (typed refusal
added as a response class, streaming gated); HF-1 FINER `weak` (Novelty 6/10, §9).
Evidence base: research runs `0088a93834c2d85f` (model + engine verification),
`25f53547f3d4cffe` (FreeToken), `db532b4453662ad4` (llama.cpp flag surface),
`020bc34a52f79cf8` (streaming landscape, PR #25294 thread).

---

## 1. Direction (kata step 1)

The frozen requirements from the committed planning set bind this module: FR-3 (private
threads resolve to the private leg, fail-closed, PI-owned policy, never silent fallback),
FR-4 (privacy marking — engine-independent, untouched by this plan), FR-5 (model briefs
with deterministic VRAM-fit arithmetic), M3/M6/M7 as above. The PI's added requirement
(this conversation): the local leg runs llama.cpp as its engine, reaches for
Colibri/FreeToken-class memory efficiency, and **zed-kask code evolves rapidly as llama.cpp
evolves** — the drift problem is the named first-class concern, not an afterthought.

## 2. Current condition (kata step 2)

Machine (probed, not assumed): Framework 16 — Ryzen AI 9 HX 370 (12C/24T), 96 GB DDR5,
8 GB RX 7700S-class dGPU (Navi 33, RDNA3, gfx1102) + 890M iGPU, ROCm kfd present,
2.9 TB free NVMe (SN850X 4 TB) plus a second NVMe (SN770M 2 TB). ollama 0.40.12 serves
small local models (≤9B class); its smallest Linux-usable tag for Qwen3.8-Flash-Next is
120 GB q4_K_M — larger than total RAM, so the registry path is closed for this model.

Verified engine landscape (research runs above): llama.cpp mainline has `--cpu-moe` /
`--n-cpu-moe` / `--override-tensor` (static expert→RAM placement) and merged PR #29887
(GPU-resident LRU cache over host-offloaded experts); PR #25294 (SSD expert streaming:
id-remap op, O_DIRECT pread pool, wave-partitional prefill, decaying-route-hotness
eviction — Colibri's learned cache, in llama.cpp) is open with maintainer convergence
intended; PR #26414 (hot-expert pinning) open; three-tier forks exist
(ServeurpersoCom `moe-stream-partition`, tested with Qwen3.8-Flash-Next). An M4 Max 36 GB
machine ran our exact model+quant (UD-Q2_K_XL + MTP draft) at ~26 tok/s decode via
streaming. One public retraction withdrew a byte-parity claim: streamed-vs-resident output
diverges on long generations (wave-GEMM FP accumulation) — semantically close, not
identical. Vulkan flash-attention on RDNA3 shipped 2026 (47% KV-VRAM cut; validated
gfx1100 — ours is gfx1102, must A/B). MTP needs the unsloth `qwen4exp/mtp` fork build.

**The honest gap:** no baseline exists yet. All performance expectations are labeled
estimates (§7) until P0 measures them. Unmeasured claims are not admitted as facts.

## 3. Target condition (kata step 3) — first increment

A private local leg that serves Qwen3.8-Flash-Next UD-Q2_K_XL (78.9 GB, top-1 82.7%)
from llama-server on this machine, where: (a) the launch is computed — placement decided
from GGUF metadata + hardware probe + policy, with every abort-class guard asserted before
the process starts; (b) the launch is version-gated — an unsupported llama.cpp build is a
typed refusal, never a wrong-flag launch; (c) the drift surface is data — a llama.cpp
release that renames flags or changes constraint semantics is absorbed by manifest +
fixture edits, zero Rust changes; (d) the baseline is measured, not assumed.

**Focus obstacle (the one):** flag-surface drift. Metrics: (1) historical flag renames
replayed against the manifest family require zero Rust diffs (H2); (2) fault-injection
fixtures for every abort-class all fail closed at emit time (H1); (3) decode/prefill/TTFT
recorded with method at P0.

## 4. The drift-absorption stack (the v3 design core)

Four layers, split by **drift rate**, not by D/P (all four are deterministic — §7):

| Layer | Lives in | What it holds | Drift rate |
|---|---|---|---|
| L1 Manifest | versioned data docs, one per engine surface (mainline pin, fork pin) | version pin; intent→flag vocabulary; closed-vocabulary constraint kinds + parameters; probe expectations | highest — most upstream churn lands here |
| L2 Constraint forms | lisp data (`hkask-lisp`, in-tree — no new dependency) | v1: the CI fixture gates for every manifest (house pattern: the same evaluator skill processes use); the reserved `expr` variant is the escape hatch for open-form constraints upstream invents | medium |
| L3 Placement compiler | Rust, the deep module | GGUF header parse + hardware probe + policy knobs → typed `LaunchPlan`; guard assertions folded in (§5); one decode home for manifests | low — the stable decision core |
| L4 Supervisor | thin Rust | spawn / probe / version-floor / stop / drain; typed refusals; stub-server test seam | low |

**Emission:** v1 renders `LaunchPlan` via a typed Rust `Display` — command line, unit
file, settings entry. **Jinja is deliberately deferred** (§10, delta 4): with one consumer
a template buys nothing over `Display`; the named promotion trigger is the **emission
unification seam** with the cloud leg's deploy-template generator (FR-2's oracle-d
surface) — when both legs exist, one registry template family renders engine launch
artifacts across legs, carrying zero policy logic (logic never lives in templates).

**The evolution claim this stack exists to satisfy:** a llama.cpp release that renames
flags or changes constraint semantics is absorbed by L1/L2 data edits plus their fixture
tests. Rust churn is confined to genuinely new decision logic (new placement intents) —
the irreducible core. The startup probe (§5) repairs the model against the installed
binary: the manifest is never trusted alone (Good Regulator).

## 5. Modules and essentialist verdicts (post-critique)

| Module | Verdict | Deletion test |
|---|---|---|
| Manifest (L1) | EARNS | delete → drift becomes Rust churn, the exact pain the PI named |
| Placement compiler (L3) | EARNS — guards folded in | delete → the complexity reappears in the operator's head (the PR-thread hand-tuning pain: cache sizing, `3*n_expert_used` aborts, OOM edges) |
| Constraint forms + fixture gates (L2) | EARNS (v1: CI gates only; `expr` reserved) | delete → manifests are unverified data; drift arrives silently |
| Supervisor (L4) | EARNS, thin | delete → PI hand-runs llama-server: no version gate, no fail-closed supervision |
| Verification — **split three ways** | startup probe: every launch; parity harness: P2 streaming admission; benchmark: P0+ measurement | delete the probe → wrong-flag launches; delete the parity harness (P2) → silent quality drift (the retraction lesson); the benchmark is the P0 harness reused, not rebuilt |
| Settings emission | EARNS, minimal | delete → the M7/M3 seam is hand-wired |
| Telemetry loop | DEFERRED to P2 | promotion trigger: ≥3 sessions of measured placement-relevant variance |
| Jinja emission | DEFERRED to the unification seam | promotion trigger: a second emission consumer |

**Placement (PI ruling, 2026-11-04):** a core kask shared service —
`kask/crates/hkask-private-inference/` — consumed by the zed-side seams (the M6
routing hook, M7 settings, a future panel). NOT an MCP server: the surface is
local-only by intent, and a core service removes the agent-callable tool surface by
construction — the P4 boundary lives in the architecture, not in approval tokens.
Everything lives under `kask/`; no D-seam, no upstream files.

**Reuse map (audited 2026-11-04 against the tree):**

- GGUF parsing — no existing tree consumer (audited); new in this crate.
- Engine-surface manifests — live under `kask/registry/private-inference/` (the
  registry's versioned-data conventions, like `registry/providers/`); one decode
  home in the crate; the fixture gates reuse the in-tree `hkask-lisp` sandbox.
- Model acquisition (next slice) — a new `acquisition` module consuming the
  `HF_TOKEN` credential through the canonical chain: the row is reinstated in the
  bridge's data services WITH the acquisition module as its read site (one companion
  change — never a credential row without a consumer, the exact anti-pattern the
  training-server retirement removed). reqwest with Range-resume + LFS sha256
  verification; the in-flight anonymous P0 download of the public repo completes
  as-is and is re-verified by the service when it lands.
- Credential chain — `write_credential` + `nudge_mcp_servers` for the llama-server
  token at the provider slot (M7): existing code, no new surface.
- Storage (M8) — the D28 SQLCipher layout via `hkask-storage`; the `mcp/` path
  prefix is re-derived for a core service by the implementation engagement.
- Upstream's `crates/language_models` llama.cpp provider (llama.app-managed) is
  noted and not used: wrong shape for the placement-controlled private leg; the
  M3 `openai_compatible` instance stays the seam.

## 6. Cybernetic adjudication

**Variety (Ashby).** Disturbance classes llama.cpp sends: (1) flag renames/additions,
(2) semantic constraint changes, (3) new behavior classes (new abort modes), (4)
performance drift, (5) streaming-mode quality drift. v1 response classes: (1) manifest,
(2) constraint kinds/forms + fixture gates, (3) launch guards + typed refusals,
(4) benchmark harness. That is **4 vs 5 → `variety_deficit`** (gate run 1, live). The
design consequence: **streaming-mode launches are unroutable in v1 routing policy** — the
private leg does not expose a mode it cannot gate (attenuation as the honest answer to a
variety deficit). With the typed refusal as a fifth response class — fail-closed refusal
is itself regulation — the gate returns **`viable`** (run 2, live).

**VSM.** S1: llama-server subprocess (and co-resident ollama). S2: port allocation
policy + the RAM budget across legs — the placement compiler's available-RAM guard must
measure **available**, not total, with headroom for the running ollama's resident set
(the M4 Max datapoint showed swap involvement; PR-thread users hit exactly this OOM
class). S3: placement compiler + supervisor (the resource bargain) + startup probe
(monitoring). S4: drift-watch — release triage to manifest-edit-only vs Rust-change with
recorded reason (runbook, step 8). S5: PI-owned policy (quality floor, routing order,
budget knobs in settings). Algedonic channel: launch refusals and parity breaches surface
typed to the PI — never silent (FR-3).

**Loops.** Startup gate: sense (version + flag probe), decide (manifest match), act
(launch or typed refusal), return (health probe) — fast, closure verified. Telemetry:
session-scale delay, P2, carries placement recomputation only (too slow for correctness —
the startup gate carries correctness). Parity: per-admission (P2), measurement with a
disclosed band, never a fabricated pass.

## 7. Claim labels (pragmatic semantics — the plan's own claims)

| Claim | Mode | Force | Provenance |
|---|---|---|---|
| "The wrapper never touches tokens" | OUGHT | Prohibition | Design (M3 ruling) |
| "Qwen3.8-Flash-Next runs locally via `--cpu-moe` on UD-Q2_K_XL" | IS | Evidence | External, verified (unsloth requirements + mainline flags) |
| "Streaming output diverges from resident on long generations" | IS | Evidence | External (the public retraction) — the reason the parity harness gates P2 |
| "The guards prevent the abort/OOM launch class" | — | **Hypothesis** (H1) | not admitted until fixtures run |
| "The manifest layer absorbs flag drift without Rust changes" | — | **Hypothesis** (H2) | testable by replaying release history |
| "Decode ~9–20 tok/s on this machine" | subjunctive | estimate | unmeasured until P0 — anchors: machine-D CPU-only 9.3, M4 Max streaming 26 |

D/P routing of the build steps is the §8 table (RR-1-validated): manifests decode SD;
compiler, forms, supervisor, emission Sem-D (all deterministic oracles named); P0
measurement and streaming/triage decisions Sem-P (verification states, never silent
passes). Every generating step carries its named deterministic gate.

## 8. Routing plan (RR-1 gate: passed, live)

| # | Step | Region | Role | Gate |
|---|---|---|---|---|
| 1 | P0 baseline: hand-launched llama-server, UD-Q2_K_XL; record decode/prefill/TTFT/agent-turn | Sem-P | agent | measurements recorded with method (band, not pass/fail) |
| 2 | Manifest schema + single typed decode home (version pin, vocabulary, constraint kinds, probe expectations, reserved `expr`) | SD | agent | decode round-trip + closed-vocabulary rejection tests |
| 3 | Placement compiler → typed LaunchPlan; guards folded in (slot floors, KV budget, available-RAM incl. ollama headroom, dense VRAM fit) | Sem-D | agent | fault-injection fixtures reproduce each abort-class; typed errors before launch |
| 4 | Constraint forms + manifest fixture gates via in-tree `hkask-lisp` in CI | Sem-D | agent | every manifest's constraints pass its fixtures |
| 5 | Supervisor: launch/probe/version-floor/drain vs a stub llama-server test seam | Sem-D | agent | integration test; probe mismatch → typed refusal naming missing flags + manifest version |
| 6 | Settings emission + routing registration | Sem-D | agent | M6 pin: marked thread resolves only within the private-leg allowlist; idempotent emission |
| 7 | Streaming-mode scope decision (P2): pinned three-tier fork + parity harness; **streaming unroutable in v1** | Sem-P | **operator** | checkpoint: approve P2 experiment with the v1 gate |
| 8 | Drift-watch cadence + release-triage runbook | Sem-P | **operator** | runbook tested on the next release; each breakage classified manifest-edit vs Rust-change |

## 9. Hypotheses (DIAG-1 gate: `ok`, live) and FINER

- **H1 (high):** guard assertions prevent the abort/OOM launch class — *prediction:*
  fault-injection fixtures derived from the PR-thread failure modes all fail closed at
  emit time; *falsifier:* any fixture reaches a runtime abort the compiler could have
  computed.
- **H2 (medium-high):** manifests + forms absorb flag-surface drift without Rust changes —
  *prediction:* replaying historical renames costs data edits only; *falsifier:* any
  historical change the typed vocabulary cannot express.
- **H3 (medium):** streaming divergence is a stable measurable band on this model —
  *prediction:* bounded band, stable enough to gate admission; *falsifier:* unbounded or
  version-dependent divergence.
- **H4 (medium, the embarrassing-if-true shape):** the available-RAM-with-ollama guard is
  required — *prediction:* total-RAM arithmetic OOMs in a measurable fraction of
  attempts, the guard reduces them to zero typed refusals; *falsifier:* the guard never
  fires (dead code).

FINER on the drift question: F8 I7 **N6** E9 R8 → HF-1 `weak` (live) — Novelty is the
weak dimension and is **not papered over**: this is an engineering hypothesis, so H2 is
reframed as the operational evolution-cost claim (measured by replay), not a novelty
claim. The framing carries the weak dimension honestly rather than inflating it.

## 10. Critique record — v2 → v3 deltas

The refined v2 was grilled (grill-me ladder), essentialist-gated (3 gates per module),
cybernetics-checked, semantics-labeled, and diagnose-disciplined. What changed:

1. **Guard core merged** into the placement compiler (essentialist G1: a separate module
   was a pass-through of the compiler's own invariants).
2. **Verification split** from one conflated "canary" into startup probe / parity
   harness (P2) / benchmark (P0+) — three mechanisms, three cadences (grill: recall).
3. **Telemetry deferred** to P2 with a named promotion trigger (essentialist G1: nothing
   to learn from until multi-session data exists).
4. **Jinja deferred** to the emission unification seam; typed `Display` in v1
   (essentialist G1: one consumer; logic-in-templates is the anti-pattern the seam must
   never violate). The PI's jinja consideration is honored as the designed seam, not
   dropped.
5. **Lisp scoped**: closed-vocabulary constraint kinds in v1; the `expr` escape hatch
   reserved; load-bearing v1 use = the CI manifest fixture gates (the house pattern).
   Don't lisp-ify what a struct field carries (essentialist: always take away).
6. **Streaming unroutable in v1** (cybernetics: variety_deficit 4v5 → attenuation; the
   typed refusal is itself a response class → viable at 5v5).
7. **Available-RAM guard** with co-resident ollama headroom (grill: edge case; the M4
   Max swap datapoint) — H4.
8. **Flag-probe made concrete**: `--version` + `--help` parsed against the manifest
   vocabulary; mismatch → typed refusal naming the missing flags and the manifest
   version; probe = Good Regulator model repair, run at CI (pinned release) and at
   launch (installed binary) (grill: mechanism).
9. **S2 coordination named**: fixed port allocation across legs; the cross-leg RAM
   budget (cybernetics: anti-oscillation).
10. **P0 first** (diagnose: an unreproduced symptom is not an admitted target — build the
    measurement harness before tuning any placement).

Edge cases on record: OOM mid-session (supervisor detects death → typed failure to the
thread, no silent fallback, restart policy operator-owned); manifest lag behind a newly
installed build (version-floor refusal, fail-closed); iGPU-vs-dGPU selection (dGPU
default; the iGPU-faster-for-qwen38 Colibri measurement is an open P2 measurement axis,
not a v1 decision); swap pressure (headroom factor in the guard).

## 11. Phases and the kata experiment record

- **P0 — baseline (step 1).** Zero code. Download gate: UD-Q2_K_XL (~79 GB) onto the
  SN850X; hand-launch with `-ngl 999 --cpu-moe -fa on --kv-cache-type q8_0 -c 65536`;
  A/B `-fa` on the gfx1102. Record decode/prefill/TTFT/agent-turn on a fixed private-
  workload prompt suite.
- **P1 — v1 wrapper (steps 2–6).** The four-layer stack, tests-first at each seam.
  Launcher backend policy (measured 2026-11-04, baseline record §5): prefer
  the HIP build — fastest on both fleet models (Qwen3.8 decode 13.0–15.0
  tok/s, Flint 23.2–30.7, vs Vulkan 10.1–12.1 / 21.2–25.6 and CPU
  7.4–8.3 / 22.7–23.1); Vulkan is the fallback and the iGPU axis.
- **P2 — gated streaming (step 7, operator checkpoint).** Pinned three-tier fork
  experiment behind the same wrapper + parity harness with the measured band; streaming
  becomes routable only when the band gates it. Telemetry loop lands when its trigger
  fires; the jinja seam when a second consumer exists.
- **P3 — upstream.** Gaps go to llama.cpp as small PRs (expert-hit telemetry hooks,
  placement presets) — never a private ggml fork (the crowded-field + rebase-burden
  ruling from the research runs). Watch FreeToken's q\*-class adaptive co-execution as
  the one technique with no landing zone.

Kata PDCA (step 4): *next step* — run P0. *prediction* — decode lands in the 9–20 tok/s
band (anchors: machine-D CPU-only 9.3 on IQ2_XS; M4 Max streaming 26 on the same
model+quant). *measurement method* — llama-server timing logs, fixed prompt suite,
`volumedetect`-style environment noted. *when to check* — at the P0 run.
`metric_after`: **measured 2026-11-04, P0 run complete.** CPU control decode
**7.4–8.3 tok/s** (200-token runs ×2, temperature 0, threads 12; agent-turn 256 tok
30.4–35.0 s) — the 9–20 prediction is **missed at the floor** on the CPU leg: the
machine-D anchor overestimated this CPU path by ~1.5 tok/s; recorded as a miss,
not smoothed. The GPU tier lands in-band and above it: Vulkan 10.1–11.5, HIP
**13.0–15.0 tok/s** (agent-turn 18.4–21.5 s). Full method + raw numbers and the
engagement evidence: baseline record §4.

**D1 implemented 2026-11-04 (finding accepted):** the engine constraint
(quantized V cache requires flash-attn) is now enforced at emit time in
`compile_placement` — the combination `kv_cache_type: q8_0` +
`flash_attn: false` is refused with a typed error
(`PlacementError::QuantizedKvRequiresFlashAttn`) naming the quantized
cache, pinned by two new tests (23 total green).

**MTP draft measured 2026-11-05 (unsloth transfer arc, measurement record
`docs/research/private-inference-unsloth-mtp-measurement-2026-11-05.md`):**
the pre-registered paired experiment on the qwen4exp/mtp fork engine
(tip 6fcaa16, HIP gfx1102) adjudicates a **GAIN** — the P2 agent-turn
median runs 16.52% faster with the shared-Q8_0 MTP draft (13.00 → 16.57
tok/s decode, 1.275x; draft acceptance 0.59–0.75), inside the
pre-declared 10–37% band and beyond the ±8% noise band, with the
consistency anchor holding exactly (13.00 = the P0 floor). Two
production constraints surfaced by the measurement itself: the
spec-verify workspace needs 1–2 GB of VRAM headroom the 8 GB card does
not have at ctx 65536 (typed ROCm OOM backtrace; the pair ran at ctx
32768, a recorded c′-class deviation), and the temperature-0 identity
instrument is confounded at reduced ctx (within-leg divergence at the
same 29-char depth as cross-leg — parity unresolved, not parity-miss).
The wrapper's draft surface (`6a1adb7699`) emits the spec flags with
Lean-proven routing-to-guard soundness (`332b6a538f`); the workspace
term in the VRAM arithmetic and the ctx policy are the named follow-ups
before MTP routes by default.

## 12. Open items for the operator

1. ~~**P0 approval**~~ — **COMPLETE 2026-11-04** (matrix ran, all four legs
   measured, record §4 filled, fleet downloaded).
2. **Step 7 checkpoint** — approve the P2 streaming experiment shape (pinned
   fork + parity band) with streaming unroutable in v1 routing.
   **APPROVED 2026-11-05** (operator, in-session): the ServeurpersoCom
   `moe-stream-partition` fork experiment — tested with this exact model,
   behind the same wrapper — with the parity harness and its measured band
   is approved as its own engagement. Streaming stays **unroutable in v1
   routing** until the band gates it (the public-retraction lesson: streamed-
   vs-resident output diverges on long generations — semantically close,
   not identical — so the band, and only the band, earns routability). The
   measured input stands as recorded (`docs/research/private-inference-
   unsloth-mtp-measurement-2026-11-05.md`: 16.5% P2 median gain, parity
   unresolved — instrument-confounded, not parity-miss). The experiment
   runs on the operator's call; the parity-instrument follow-up (a
   fresh-slot protocol before any identity claim) precedes any band
   adjudication.
3. **Drift-watch ownership** — operator-triaged on each llama.cpp release, or delegated
   with typed escalation; the runbook (step 8) is written either way.

## 13. Generalization (PI question 2026-11-04, post-matrix)

The placement compiler is machine-adaptive by design (live hardware probe,
guard arithmetic over detected numbers, fail-closed defaults). The honest
three gaps the PI identified, now accepted as next slices:

1. **Backend auto-detection is missing** — the probe finds amdgpu but the
   policy hardcodes the HIP build. On a non-AMD machine the HIP build
   doesn’t exist and the fallback is manual. Next slice: a `GpuBackend`
   probe that tries HIP → Vulkan → CPU in order.
2. **The VRAM walk covers only amdgpu sysfs** — on NVIDIA or Apple
   Silicon, `probe_vram` returns `None` → `VramDenseFit` refuses →
   CPU-only. Correct but suboptimal. Next slice: multi-vendor probe
   (nvidia-smi path, unified-memory detection).
3. **Per-model data (the PLE pin, expert counts, recommended KV) is
   policy-hardcoded** — the pin `per_layer_token_embd=CPU` is correct for
   Qwen3.8-Flash-Next but not for Gemma or Muse. Next slice: the **model
   brief** (M9’s deterministic core) — a per-model TOML in the registry
   carrying the pin pattern, expert metadata, recommended settings.

**Implementation order (the next three slices):** model brief (M9)
→ backend auto-detect → multi-vendor VRAM probe. Each is data-first
(registry TOML + Rust decode home), test-gated, and fails closed.
