# Continuation prompt — GPU/Vulkan leg of the private-inference local service

**Handoff target:** any agent with repo access and no conversation history.
**Composed:** 2026-11-04, from the private-inference local-leg session.
**Read first:** the worktree-root `.rules` (required) and
`kask/docs/plans/private-inference-local-leg-llama-wrapper-plan-2026-11-04.md` (the design
this work extends — §5 carries the placement ruling and reuse map).

---

## Mission

Bring the GPU tier of the private-inference local service live on this Framework laptop:
build llama.cpp with the Vulkan backend (primary) and HIP (fallback), auto-detect the
dGPU's VRAM in the core service, run the measured launch-policy A/B matrix on the real
weights, and wire the measured recommendation into the placement compiler's GPU policy.
Every step is gated; every measurement is recorded with method; every surprising output
is re-verified byte-level before it is believed.

## Verified ground truth (checked against the tree and the machine, 2026-11-04)

**Machine:** Framework 16 — AMD Ryzen AI 9 HX 370 (12C/24T), 96 GB DDR5, RX 7700S-class
dGPU 8 GB (Navi 33, RDNA3, **gfx1102**), Radeon 890M iGPU (RDNA 3.5). **ROCm 7.2.0 is
installed** (`/opt/rocm`, `hipcc` on PATH) — HIP builds need no sudo. Vulkan **loader**
1.4.341 present (`vulkaninfo` works) but **no Vulkan dev headers and no glslc** are
installed, and there is **no passwordless sudo** — apt installs are operator-gated.

**Repo:** `/home/mdz-axolotl/Clones/zed-kask` (a SHARED tree — pathspec commits only,
never a bare `git commit`, never a working-tree-wide mutation). The core service crate is
`kask/crates/hkask-private-inference/` (lib `hkask_private_inference`; modules
`src/local_leg/{gguf,manifest,placement,launch}.rs`; `tests/real_gguf.rs` env-gated by
`HKASK_P0_GGUF_DIR`) — 14/14 tests green, clippy-clean as of handoff. The engine-surface
manifest lives at `kask/registry/private-inference/llama-cpp-mainline.toml` (fixture-
gated; the decode home is `manifest.rs` — one home, never inline). The architecture
ruling (dated amendment on
`kask/docs/plans/private-inference-mcp-architecture-plan-2026-10-30.md`): private
inference is a **core kask shared service, NOT an MCP server** — keep it that way.

**Engine:** llama.cpp mainline cloned at `~/models/llama.cpp` (v0.9.0-dev, commit
9e2a04a at build time). A CPU-only build exists at `~/models/llama.cpp/build`
(`llama-server`, `llama-gguf` — both working). Leave it untouched; new backends build in
separate build dirs (`build-vulkan`, `build-hip`).

**Weights:** downloading to `~/models/qwen38-flash-next/UD-Q2_K_XL` — the directory name
is hyphenated; match the HF spelling exactly. Four shards ≈ 79.6 GB total; **shard 1
(10.9 MB) is metadata-only — zero tensors — and the crate's parser handles that, pinned by
test.** Shards 2–4 are in flight via a resumable script
(`~/models/qwen38-flash-next/download_rest.sh`); DO NOT disturb the running download.
Verified model metadata, parsed live from the real header shard: `expert_count=512,
expert_used=10, block_count=48, context_length=262144`.

**Research-verified anchors** (recorded research runs, 2026-11-04):
- The GPU placement for this machine: `-ngl 999 --cpu-moe` = dense/attention/KV on the
  dGPU, routed experts resident in RAM. Vulkan flash-attention on RDNA3 shipped 2026
  (~47% KV-VRAM cut, near-lossless) but was **validated on gfx1100; ours is gfx1102**
  with third-party degradation reports — the `-fa on/off` A/B on this card is
  load-bearing, not optional.
- Colibri measured an iGPU (780M-class) *faster* than CPU for this exact model family via
  its Vulkan expert tier — the 890M leg is an exploratory measurement axis, not a v1
  decision.
- Expert streaming (llama.cpp PR #25294-class) is P2 and **unroutable in v1** — out of
  scope; do not enable or plan for it.

## The work (phases, each gated)

1. **Operator gate.** The Vulkan build needs two packages you cannot install: ask the
   operator to run `sudo apt install libvulkan-dev glslc` (verify the glslc package name
   with `apt-cache policy glslc` on 26.04; `glslang-tools` is the fallback provider if
   the name differs). While waiting — or if declined — proceed on the HIP path. Record
   which paths exist at the end.
2. **Builds.** `build-vulkan`: `cmake -B build-vulkan -DCMAKE_BUILD_TYPE=Release
   -DGGML_VULKAN=ON` then build `llama-server llama-gguf`. `build-hip`: `-DGGML_HIP=ON`
   with `-DAMDGPU_TARGETS=gfx1102`. Long builds run detached — write a script, launch
   `setsid nohup <script> > <log> 2>&1 < /dev/null &` (a plain `&` chain gets reaped by
   the tool shell; observed live this session), then poll the log. A HIP build failure
   on gfx1102 is a recorded finding, not a blocker — Vulkan is the primary path.
3. **VRAM auto-detection in the core service** (the usability core of this leg).
   amdgpu exposes VRAM at `/sys/class/drm/card*/device/mem_info_vram_total`. Extend
   the hardware probe in `placement.rs`: `HardwareInventory`'s VRAM field gains a
   source (`Sysfs { card_path, bytes } | Declared { bytes }`) with provenance carried;
   probe all amdgpu cards, take the largest as the dGPU candidate (the 890M's carve-out
   is small; verify rather than assume which card is which). Fixture-based unit tests
   for the sysfs parse; an env-gated live test. Guard semantics stay fail-closed: a GPU
   offload plan without detectable VRAM is a typed refusal — existing tests pin this;
   extend for the detected case. House rules: no `unwrap()` in prod, `expect()` with
   messages in tests only; defaults in `Default` impls only; `./script/clippy` is the
   gate, never bare `cargo clippy`; run cargo scopes sequentially.
4. **Measurement matrix** (once the download completes — verify first with the live
   shard test: `HKASK_P0_GGUF_DIR=/home/mdz-axolotl/models/qwen38-flash-next/UD-Q2_K_XL
   cargo test -p hkask-private-inference --test real_gguf -- --ignored --nocapture`; the
   strict guard proves shard completeness). Legs: (a) CPU-only control (existing
   build), (b) dGPU Vulkan `-fa on`, (c) dGPU Vulkan `-fa off`, (d) HIP if it built,
   (e) optional exploratory iGPU Vulkan leg. Per leg: `-ngl 999 --cpu-moe
   --kv-cache-type q8_0 -c 65536` adapted as evidence demands (record every deviation),
   a fixed private-workload prompt suite, and measured: decode tok/s, prefill tok/s,
   time-to-first-token, resident RAM, agent-turn latency. **GPU engagement is evidence,
   not assumption:** read the server's own layer-offload report at load; a leg showing
   0 offloaded layers did not engage the GPU — record that, never present it as a GPU
   number. llama-server binds loopback; leave no servers running at the end.
5. **Manifest + policy updates.** Extend the registry manifest for the measured GPU
   surface (update `engine_version_pin` to the actual builds; the flag vocabulary
   already carries `-ngl`/`--cpu-moe`). Update `PlacementPolicy` GPU defaults to the
   measured recommendation only if the measurement supports it. The placement compiler
   emits the plan; launch by hand for P0 as before.
6. **Record + report.** Write
   `kask/docs/research/private-inference-local-leg-p0-baseline-2026-11-04.md` (create
   it; the CPU leg is the control) with environment, method, raw numbers, and
   deviations. Fill the kata `metric_after` in the local-leg plan §11 from the
   measurement — never from a plan. Report net lines, the gates run, and uncommitted
   state (say "uncommitted" plainly; commit only on operator instruction, pathspec).

## Boundaries

No private ggml fork — ever. Streaming stays P2/unroutable. Do not reopen M3/M6/M7 or
touch the FR-4 privacy marking. Do not touch upstream zed files (all work under `kask/`).
Do not reinstall or replace ollama. Do not disturb the running download. Two long-command
display glitches were observed this session (a `ls -d` NotFound disproved by a byte-level
`cat -A` re-check) — re-verify surprising outputs byte-level before acting on them.

## Acceptance criteria (each can fail)

1. A Vulkan or HIP `llama-server --version` reports the backend, from its build dir.
2. A real launch of Qwen3.8-Flash-Next UD-Q2_K_XL with **>0 layers offloaded to the
   dGPU** (server's own report as evidence), through a `LaunchPlan` the placement
   compiler emitted with **detected** (not declared) VRAM.
3. The `-fa on/off` A/B table exists for gfx1102 with method; the recommendation is
   measured, not assumed.
4. VRAM probe: fixture unit tests green; live test green on this machine; a GPU-offload
   plan without detectable VRAM still fails closed with a typed error.
5. `cargo test -p hkask-private-inference` green; `./script/clippy
   -p hkask-private-inference` green; the live real_gguf test green on the completed
   shard set.
6. The baseline record exists (cite `wc -l`), the kata `metric_after` is filled from
   measurement, and no server processes are left running.
