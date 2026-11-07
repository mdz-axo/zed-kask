# llama.cpp experimental-surface patches

The operator ruling 2026-11-06 allows exactly TWO llama.cpp surfaces:
ollama's bundled engine, and the L1-pinned mainline owned by zed-kask
(`llama-cpp-mainline.toml`, commit 9e2a04a). The experimental engine trees
that once lived under
`{artifacts}/private-inference/engines/{qwen4exp-mtp,serveurperso-moe-stream-partition}/`
are GONE — their capability is preserved HERE, as patches at their
BUILD_PIN shas (requirement 4 of the consolidation plan REV 2):

| Patch | BUILD_PIN (tip) | Recorded base | Capability |
|---|---|---|---|
| `llama-cpp-qwen4exp-mtp-6fcaa16f….patch` | 6fcaa16f… | bb3c853 (upstream at the branch's master merge) | the MTP draft-head experiment engine (danielhanchen fork, `qwen4exp/mtp` = pr-28243-rebase) |
| `llama-cpp-serveurperso-moe-stream-7174388….patch` | 7174388895… | 9a4843c (upstream) | the MoE stream-partition experiment (ServeurpersoCom fork, PR #25294) — three-tier expert streaming |
| `llama-cpp-sys4-0004-exact-decode-lifecycle-hooks.patch` | 9e2a04a (the canonical mainline pin) | llama-cpp-sys-4-0.11.0's vendored llama.cpp commit (upstream eugenehp/llama-cpp-rs patch) | the capture leg's decode lifecycle hooks — llama-cpp-4's tensor transactions; staged at build time by the in-repo fork `kask/crates/llama-cpp-sys-4`, verified apply-clean against the pin 2026-11-07 |
| `llama-cpp-sys4-0005-fail-closed-eagle3-process.patch` | 9e2a04a (the canonical mainline pin) | same | the eagle3 speculative-process fail-closed guard; same fork staging |

P5 note (2026-11-07): the upstream sys crate's third always-staged patch
(0003-exact-speculative-state) does NOT apply to the pin — it patches the
vendored commit's `common/speculative.cpp` — and is dropped in the fork.
Its only load-bearing effect for this workspace (the bool return of
`common_speculative_set_state`) is carried instead by the fork's
`mtp_shim.cpp` port to the pin's void API, keeping the pin's source 100%
upstream; the shim's own get_state round-checks retain the failure
detection.

Each patch is the net tree delta of the branch tip against its recorded
upstream base (extraction receipt inside the patch header). Each was
verified 2026-11-06 to `git apply --check` cleanly against its recorded
base (checkout base → apply-check, in-tree, before the tree's deletion).

Revival: apply onto the recorded base, then rebase onto the canonical
mainline pin when the thread needs it. A future re-pin is a manifest edit
(the drift-absorption layer owns it) — never an edit to these patch files'
recorded bases.

The serveurperso surface's flag vocabulary and constraints remain recorded
at `../llama-cpp-serveurperso-moe-stream.toml` (an EXPERIMENT manifest,
never the shipped surface); streaming stays unroutable in the placement
compiler until the band earns routability.
