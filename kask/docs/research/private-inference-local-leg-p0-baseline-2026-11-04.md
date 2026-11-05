# Private-inference local leg — P0 baseline record (2026-11-04)

Status: **in progress**. Environment, build state and the VRAM
auto-detection capability below are recorded from live probes run
2026-11-04. The measurement matrix (§4) and the measured GPU policy
recommendation (§5) are pending the weights download and the operator
apt gate; numbers land here only from measurements, never from plans.

## 1. Environment (verified live 2026-11-04)

Machine: Framework 16 — AMD Ryzen AI 9 HX 370 (12C/24T), 96 GB DDR5.

GPUs (amdgpu sysfs, `vendor:device`, `mem_info_vram_total` — read via
`cat`, values byte-verified against the core service's live probe):

| drm node | PCI id | VRAM (bytes) | identity |
|---|---|---|---|
| `/sys/class/drm/card1/device` | `0x1002:0x7480` | 8,573,157,376 | RX 7700S-class dGPU (Navi 33, gfx1102) |
| `/sys/class/drm/card2/device` | `0x1002:0x150e` | 536,870,912 | Radeon 890M iGPU (RDNA 3.5, gfx1150) |

ROCm 7.2.0 installed (`/opt/rocm`, `hipcc` on PATH); `rocminfo` lists
gfx1102 and gfx1150. Vulkan loader 1.4.341 present; `libvulkan-dev` and
`glslc` NOT installed — operator-gated (§3).

Engine: llama.cpp mainline at commit `9e2a04a` (v0.9.0-dev), cloned at
`~/models/llama.cpp`. CPU-only build at `build/` (the control leg).
GPU builds: `build-hip/` (gfx1102) **completed 2026-11-04**:
`BUILD_HIP_DONE`; `llama-server --list-devices` reports
`ROCm0: AMD Radeon RX 7700S (8176 MiB, 8140 MiB free)` and
`ROCm1: AMD Radeon 890M Graphics (46763 MiB, 46759 MiB free —
GTT-backed, not the 512 MiB BIOS carve-out)`; the binary links
ROCm 7.2 (`libamdhip64.so.7`, `librocblas.so.5`). `build-vulkan/`
**completed 2026-11-04**: `BUILD_VULKAN_DONE`; `--list-devices` reports
`Vulkan0: AMD Radeon RX 7700S (RADV NAVI33) (8176 MiB, 8149 MiB free)`
and `Vulkan1: AMD Radeon 890M Graphics (RADV STRIX1) (47275 MiB,
45783 MiB free)` — one build serves the dGPU legs and the exploratory
iGPU leg; links `libggml-vulkan.so.0` plus a cmake-fetched loader
copy under `~/models/vulkan-prefix/` (device enumeration verified at
runtime). Build scripts:
`~/models/build-hip.sh`, `~/models/build-vulkan.sh` (both detached via
`setsid nohup … > log 2>&1 < /dev/null &`).

Weights: Qwen3.8-Flash-Next UD-Q2_K_XL, four shards ≈ 79.6 GB, at
`~/models/qwen38-flash-next/UD-Q2_K_XL` — download in flight at record
time (31 GB on disk at 12:59: shard 1 complete and metadata-only,
10.9 MB, zero tensors — the crate's parser handles that, pinned by
test; shard 2 at 33.2 GB and writing; shards 3–4 pending). Header-shard
metadata parsed live at handoff: expert_count=512, expert_used=10,
block_count=48, context_length=262144.

## 2. VRAM auto-detection (capability change, 2026-11-04)

`HardwareInventory.vram: Option<VramSource>` —
`Sysfs { card_path, bytes }` (detected, primary) or
`Declared { bytes }` (caller estimate). This **replaces** the
declared-only field `vram_bytes_declared: Option<u64>`; `probe_hardware()`
now probes RAM (`/proc/meminfo`) and VRAM in one pass instead of
hardcoding `None`.

Probe semantics: walks `/sys/class/drm/card*/device/mem_info_vram_total`
(the node's presence is the amdgpu marker — connectors and render nodes
are name-filtered), takes the largest card as the dGPU candidate
(largest-wins verified: the 7700S's ~8 GiB vs the 890M's 512 MiB BIOS
carve-out), carries the winning card path as provenance, fails loud on a
malformed node, and leaves `None` for machines without amdgpu — the
absence the `VramDenseFit` guard fails closed on (unchanged refusal,
pinned by test).

Gates run 2026-11-04:

- `cargo test -p hkask-private-inference` — **19 passed, 0 failed**
  (14 at handoff + 4 new probe tests: largest-wins over a two-card
  fixture, absence, malformed-node failure, detected-VRAM plan compile;
  +1 pin for the explicit `--flash-attn off` emission added for the A/B).
- `HKASK_P0_SYSFS=1 cargo test -p hkask-private-inference --test
  live_sysfs -- --ignored --nocapture` — **1 passed**; live output:
  `detected dGPU VRAM: 8573157376 bytes at /sys/class/drm/card1/device`.
- `./script/clippy -p hkask-private-inference` — clean (exit 0).

The live real_gguf test passed on the completed shard set 2026-11-04:
full-set meta `1224 tensors, expert_bytes=42.87 GiB,
dense_bytes=31.23 GiB` — the real dense class is ~4.5× the fixture
estimate (6.9 GB), which motivated the PLE CPU-pin policy (§4).

## 3. Operator gates

- **Vulkan dev surface (was blocking legs b/c): PASSED 2026-11-04.**
  Operator ran `sudo apt install libvulkan-dev glslc`; verified
  installed (`ii`: `glslc 2026.1-1`, `libvulkan-dev 1.4.341.0-1`),
  `/usr/bin/glslc` functional (shaderc 2026.1-1), headers at
  `/usr/include/vulkan/`. The detached build completed — see §1.

## 4. Measurement matrix (pending — legs and method fixed in advance)

Evidence rules: GPU engagement is read from the server's own
layer-offload report at load — a leg showing 0 offloaded layers is
recorded as non-engaged, never presented as a GPU number. llama-server
binds loopback; no servers left running after a leg.

Legs (flags adapted as evidence demands; every deviation recorded here):

| leg | binary | flags | status |
|---|---|---|---|
| a — control | `build/bin/llama-server` | CPU-only (`-c 65536`, KV q8_0, `-fa on`, threads 12) | **measured** |
| b — dGPU Vulkan | `build-vulkan/bin/llama-server` | plan b: `-ngl 999 --cpu-moe --override-tensor per_layer_token_embd=CPU -fa on`, KV q8_0 | **measured** |
| c′ — dGPU Vulkan | `build-vulkan/bin/llama-server` | as b, `-fa off` + `--cache-type-v f16` (deviation D1) | **measured** (adapted) |
| d — dGPU HIP | `build-hip/bin/llama-server` | plan d: as b on the HIP build | **measured** |
| e — iGPU Vulkan (exploratory) | `build-vulkan/bin/llama-server`, Vulkan1 device | same shape | not run (optional axis) |

Plan emission (the launch path): each leg launches from a `LaunchPlan`
the placement compiler emitted over the LIVE hardware probe
(sysfs-detected VRAM) and the full shard-set metadata — never
hand-invented flags:

```sh
HKASK_P0_GGUF_DIR=~/models/qwen38-flash-next/UD-Q2_K_XL \
  cargo test -p hkask-private-inference --test emit_leg_plans -- --ignored --nocapture
```

The leg's build-dir binary is prefixed at hand-launch time (the plan's
`binary` stays the manifest's `llama-server`). Compiler emission change
(2026-11-04): `--flash-attn` is now always explicit (`on`/`off`),
replacing omit-when-false — both A/B legs must be emittable plans.

Measurement harness (pinned 2026-11-04): `~/models/measure-leg.sh`,
sha256 `7db74fa1ccb0dc92f995faf791ad6b62cc11e79625c7797c6c03cefdf0e41869`,
`bash -n` clean. One call per leg: launches the emitted plan verbatim,
polls `/health` (20-minute bound), captures the server banner + load
report (the GPU-engagement evidence) + `/metrics`, then runs the fixed
suite — warmup (unmeasured), ~3.7K-token single-line prefill probe ×2
(TTFT via `curl time_starttransfer`; prefill tok/s from usage),
200-token decode probe ×2 (tok/s from content-chunk timing),
256-token non-streamed agent-turn ×2 (wall latency +
completion_tokens) — records VmRSS/VmHWM + `free -m`, then kills the
server (EXIT-trap guarded). The prompt suite is embedded verbatim in
the script; probes run at temperature 0. Artifacts land in
`~/models/legs/<leg>-*`.

### Results (2026-11-04, the matrix run)

Method recap: every leg launched from a compiler-emitted plan over the
sysfs-detected VRAM budget (8,573,157,376 bytes at card1) and the real
full-set meta (`expert_bytes=42.87 GiB`, `dense_bytes=31.23 GiB`, 1224
tensors), through the sha256-pinned harness; temperature 0; threads 12;
server `n_slots=4, n_ctx_slot=65536`; 2 runs per probe.

| leg | decode tok/s (r1/r2, 200 tok) | agent-turn s (r1/r2, 256 tok) | cold prefill s (r1, 2892 tok) | VmRSS |
|---|---|---|---|---|
| a — CPU control | 7.4 / 8.3 | 35.0 / 30.4 | 56.5 | 67.1 GB |
| b — Vulkan `-fa on` | 10.1 / 11.5 | 27.6 / 21.5 | 34.6 | 51.4 GB |
| c′ — Vulkan `-fa off` (V f16) | 10.4 / 12.1 | 27.3 / 21.7 | 36.6 | 51.5 GB |
| d — HIP `-fa on` | **13.0 / 15.0** | **21.5 / 18.4** | 22.1 | 51.5 GB |

GPU engagement — the acceptance evidence, verbatim from the server and
the driver: `load_tensors: offloading output layer to GPU`,
`offloading 47 repeating layers to GPU`, **`offloaded 49/49 layers to
GPU`**, `ROCm0 compute buffer size = 1141.16 MiB` (engagement probe at
`-lv 5`, leg-d config), and the amdgpu driver's own reading during the
live server: **`mem_info_vram_used` = 7,394,054,144 of 8,573,157,376
bytes (86% of the card)**. Corroborated: VmRSS drops 67.1 → 51.4 GB
CPU→GPU and every GPU leg out-decodes the control. Both GPU backends
log `layer 3 is assigned to device … but fused top-k mask is assigned
to device CPU` — the fused-op gap recorded verbatim (D5).

Deviations and harness limits, recorded:

- **D1 (engine constraint, load-bearing for the A/B):** `-fa off`
  with a quantized V cache is refused by the engine (`quantized V
  cache requires flash_attn to be enabled`) — leg c as planned was
  unloadable; c′ runs `-fa off` with `--cache-type-v f16` (K stays
  q8_0). The A/B therefore measures the real-world bundle
  (fa-on+KVq8_0 vs fa-off+Vf16). On gfx1102 it is a throughput tie
  within run noise (10.1–11.5 vs 10.4–12.1 tok/s); no degradation
  observed at ctx 65536. `-fa on` additionally buys the ~47% KV cut
  and is REQUIRED for quantized V — recommendation: **`-fa on`**,
  measured, not assumed.
- **D2 (slot cache):** prefill r2 hits the server's slot KV cache
  (identical prompt, same slot) — not a prefill measurement; the
  comparable prefill number is r1 (cold, includes expert page-in).
- **D3 (SSE framing):** the content-chunk counter reads 1 on this
  server's SSE shape; token counts come from `usage` fields and the
  derived tok/s uses those.
- **D4:** the server provisions `n_slots=4` at `n_ctx_slot=65536` —
  noted, not adapted (identical across legs, so comparisons hold).

The `-fa on/off` A/B on gfx1102 is load-bearing, not optional: Vulkan
flash-attention shipped 2026 validated on gfx1100, with third-party
degradation reports for gfx1102 — the recommendation must be measured.

## 5. Measured GPU policy recommendation

From §4's matrix (2026-11-04): **the GPU tier is the default placement
for this machine.** HIP serves 13.0–15.0 tok/s decode (agent-turn
18.4–21.5 s per 256 tokens) — ~1.75× the CPU control and ~1.3× Vulkan;
Vulkan 10.1–11.5 tok/s; CPU control 7.4–8.3. `-fa on` is recommended:
throughput tie with `-fa off` on gfx1102, required for quantized V
cache, ~47% KV-VRAM cut, no degradation observed at ctx 65536.

Landed with the measurement (phase 5): `PlacementPolicy::default()`
carries `n_gpu_layers: 999` (the measured tier; fail-closed where VRAM
is undetectable, via `vram_dense_fit`); `engine_version_pin` records
the actual builds (commit 9e2a04a; CPU / Vulkan / HIP gfx1102); the PLE
pin stays model-specific in the caller — `pin_to_cpu` is never a
default (an unmatched pattern would emit a dead `--override-tensor`
on models without per-layer embeddings). Streaming (PR #25294-class)
stays P2/unroutable in v1 — not enabled, not planned for.

## 6. Model fleet (2026-11-04, research runs df2d619d64dc29b4 +
8d71a88a57a0dc74)

Fleet additions verified by web research + HF API file trees (evidence in
the run ledger); maker names corrected from the request:

- **Qwen3.8-Flash-Next UD-Q2_K_XL** (79.6 GB, downloading) — the P0
  baseline model.
- **Flint — by Recursive** (recursive.com, HF org `recursivesi`; not
  "Recursion AI"), Apache 2.0: 39.8B MoE, ~2.5B active, 262K context,
  agentic coding + tool use. Artifact: `unsloth/flint-GGUF`
  `flint-UD-Q4_K_XL.gguf`, 25,054,688,704 bytes, single file. llama.cpp
  mainline 9e2a04a carries the flint arch (`llama-arch.cpp`,
  `llama-model.cpp`) — all three builds serve it. Download sequenced behind the Qwen download in the fleet queue
(`~/models/download-queue.sh`, log `~/models/fleet-queue.log`):
measured, parallel HF streams split the ~10 MB/s pipe (Qwen 10.5 →
3.1 MB/s while Flint ran at 7.0 MB/s over a 75s window), so each model
waits its turn; every fetch resumes from its partial file via
`curl -C -`.
- **Beam — by Reflection AI**, Apache 2.0: 501B MoE, 23B active, 1M
  context. The smallest existing quant, `unsloth/Beam-1.0-501B-A23B-GGUF`
  UD-IQ1_S, totals **103,059,961,380 bytes** (49,583,532,032 +
  49,439,126,816 + 4,031,348,512 + a 5,954,020-byte metadata-only header
  shard — the same zero-tensor first-shard pattern the crate's parser
  pins for Qwen). That is ≈ the machine's entire 96 GB of physical RAM
  in weights alone, before KV/compute/OS overhead. **No Beam quant can
  serve on this machine in v1** (expert streaming is P2/unroutable, plan
  ruling). llama.cpp 9e2a04a carries the arch, so parse-level and future use
  remain possible. **Operator decision (2026-11-04): Beam is skipped
  for this machine** — the size-class alternates below were chosen
  instead; the fit verdict stands (revisit only if RAM changes).
- **Gemma 4 31B — by Google DeepMind**, Apache 2.0: dense 31B,
  multimodal, reasoning mode, 39 on the AA Intelligence Index (the
  highest-AA US open model that fits). Artifact:
  `unsloth/gemma-4-31B-it-GGUF` `gemma-4-31B-it-UD-Q4_K_XL.gguf`,
  **18,822,970,304 bytes**, single file (Q5_K_XL 21.9 GB / Q6_K_XL
  27.5 GB are one-file upgrades if quality disappoints; image input
  needs the optional `mmproj-F16.gguf`, 1.2 GB — not fetched).
  `gemma` arch in our 9e2a04a build (12/28 grep hits).
- **Muse Glimmer 30B — by Meta Superintelligence Labs**, Apache 2.0:
  dense 30B agentic model distilled from Muse Spark (vision encoder
  and DFlash speculative drafter exist as optional one-file adds —
  not fetched). Artifact: `unsloth/Muse-Glimmer-30B-GGUF`
  `Muse-Glimmer-30B-UD-Q4_K_XL.gguf`, **15,878,222,368 bytes**, single
  file. `glimmer`/`LLM_ARCH_MUSE` in our build (mainline support
  merged; community quants target commit 62bf73d ≤ 9e2a04a).

Fleet queue: Qwen (in flight) → Flint → Gemma 4 31B → Muse Glimmer 30B,
driven by `~/models/download-queue.sh` (resumable per file; failures
logged to `~/models/fleet-queue.log` and skipped, never silent).
Hardened and relaunched by a concurrent stream at 15:08:40 (log line
"restart-aware, size-verified"; pid 3796072 replacing this stream's
14:57 instance): the queue now fires only on the `ALL_SHARDS_DONE`
completion marker — a Qwen download death keeps it waiting for a
restart instead of splitting the pipe — and each fetched file is
byte-verified against the HF-listed size. Verified sound by this
stream: fetch order, URLs and expected sizes unchanged
(25054688704 / 18822970304 / 15878222368), one instance running, no
parallel fetches.

**Download incident, corrected record (2026-11-04):** shard 2
completed 13:27 (49,732,486,272 bytes — byte-exact vs the HF listing)
and shard 3 completed 13:29 (1,037,181,344 bytes — byte-exact); the
stream then on shard 4 degraded to ~0.7 MB/s. A restart was executed
on a misread — the "stall" observation had keyed on completed shard
2's frozen size, not the active shard 4. The restart was resume-safe
by design (`curl -C -`): shards 2/3 re-verified as instant no-ops,
shard 4 resumed from its partial with zero bytes lost, and the fresh
connection recovered full rate (measured 689,262,592 bytes/min ≈
11.5 MB/s over a 60s window at 14:58). Shard 4 (28,800,138,432 bytes
listed) ETA ~15:35 at that rate.
