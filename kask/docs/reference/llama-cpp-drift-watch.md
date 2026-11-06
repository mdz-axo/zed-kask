# llama.cpp drift-watch — release-triage runbook

**Surface:** the private-inference local leg's engine — llama.cpp as launched by the
placement compiler behind `hkask-private-inference`'s manifest family.
**Design home:** `kask/docs/plans/private-inference-local-leg-llama-wrapper-plan-2026-11-04.md`
(§4 the drift-absorption stack; §8 step 8 — this runbook; §12 item 3 the ownership
ruling). **Baseline:** the P0 matrix and the MTP measurement records under
`kask/docs/research/`.

**Ownership (plan §12 item 3 — the operator's ruling, 2026-11-06: DELEGATED with
typed escalation, under the operator's "proceed with the proposed follow-ups"
direction):** the agent runs the triage procedure below on each llama.cpp
release; the typed refusals are the escalation signal (a `CommitMismatch` names
both commits, a `MissingFlags` names the exact spellings, a failing fixture
names the constraint), and the Rust-change classification escalates to the
operator as a placement finding with the classification attached. The two
operator-owned pieces stay the operator's in either mode: the adopt-iff-verified
re-pin gates (step 3) and the swap click (step 4 — launch = swap; the operator's
click is the intent). This runbook still serves either mode; the ruling names
who runs the steps.

## Why the watch exists

Flag-surface drift is the local leg's named focus obstacle (plan §3). The stack is
split by drift rate so that most upstream churn lands in data, not Rust:

| Layer | Lives in | Absorbs |
|---|---|---|
| L1 manifest | the embedded TOML surfaces in `hkask-private-inference/src/local_leg/manifest.rs` (`SHIPPED_MANIFEST_TOML`, the prebuilt fork surface) | flag spellings, endpoint paths, probe expectations, constraint parameters |
| L2 constraint forms | manifest fixture rows (in-tree lisp data, `run_fixtures`) | open-form constraints upstream invents (the reserved `expr` variant) |
| L3 placement compiler | Rust (`compile_placement`, `placement.rs`) | only genuinely new decision logic — the irreducible core |
| L4 supervisor | `probe.rs` + the supervisor | typed refusals at the launch boundary |

## The standing gates (what fires without anyone watching)

1. **The version pin is exact-commit (D-S2).** The manifest's `VersionPin`
   (`manifest.rs`) pins the verified commit; `probe.rs::probe_binary` refuses any
   other binary with `ProbeError::CommitMismatch { binary_commit, pinned_commit }` —
   both commits named. A rebuilt or upgraded engine NEVER launches until the
   manifest is re-pinned: drift cannot silently reach the running leg.
2. **The flag surface is probed.** `ProbeError::MissingFlags { missing,
   manifest_commit }` names the exact missing spellings — the extension-rename
   drift class the probe's own falsifier found (`--flash-attn` → `--flash-attention`).
3. **The fixtures gate the constraints.** Every manifest's constraint set runs its
   fixtures in the crate's test suite (`run_fixtures`, manifest.rs tests) — a
   renamed flag fails a fixture in CI, not a launch on the machine.

## The triage procedure (each llama.cpp release)

1. **Read the release** (tag, release notes, the diff against the pinned commit) for:
   flag renames; new/removed launch flags; constraint-semantic changes (VRAM/KV/
   quant rules); build-surface changes (HIP/Vulkan build layout); endpoint changes
   (`/health`, `/v1/models`).
2. **Classify each observed change** — the step-8 discipline, each breakage
   classified manifest-edit vs Rust-change:
   - **Manifest edit (zero Rust):** flag spelling, endpoint path, probe expectation,
     constraint parameter → edit the manifest TOML in `manifest.rs` + its fixture
     rows; the historical-rename replay test must still pass (H2's claim: replaying
     renames costs data edits only).
   - **Constraint form (zero Rust):** a new open-form constraint → the reserved
     `expr` variant (L2), with a new fixture row.
   - **Rust change (recorded finding, not routine):** genuinely new decision logic
     (a new placement intent) → `compile_placement` + a typed error + a
     fault-injection fixture. A release that forces this class is escalated as a
     typed placement finding, never hand-edited in place.
3. **Re-pin only after verification (D-S3 adopt-iff-verified):** update
   `[version_pin]` (commit + version) in the manifest, then run the gates —
   `cargo test -p hkask-private-inference` (fixtures + manifest decode), and the
   live launch gate (`HKASK_P1_LIVE=1` `live_supervisor`, or the Serving page's
   launch): liveness, endpoint, health, served-model identity — never the file alone.
4. **Swap is the explicit click.** Launching the new build stops and drains the
   current server first (launch = swap, plan §13); the operator's click is the
   intent.
5. **Append a row to the triage log** below (release id, classified changes,
   manifest-edit vs Rust-change, the re-pinned commit, the gates run).

## The escalation path (delegated mode)

A delegated watch escalates through the typed refusals themselves: the probe's
`CommitMismatch` / `MissingFlags` errors and a failing fixture ARE the signal —
they name both commits and the exact missing spellings, so an escalation carries
its own evidence. Anything that cannot be absorbed by manifest/form edits (the
Rust-change classification) escalates to the operator as a placement finding with
the classification attached.

## Triage log

| Date | Release | Classification | Manifest edits | Rust changes | Re-pinned commit | Gates run |
|---|---|---|---|---|---|---|
| — | *(the next release is this runbook's first live test — plan §8 step 8's gate)* | | | | | |

## Scope boundary

Model-weight drift (a changed GGUF) is not engine drift — it re-runs the VRAM fit
and the fleet brief, not this watch. The MTP/streaming forks' parity harness is the
P2 experiment's own engagement (operator-called); a fork re-pin follows this same
procedure against the fork's manifest surface. The provider-cloud leg has its own
deployment drift surface (FR-2).
