# Code Review — 2026-08-07 Kask Cleanup Session

**Review date:** 2026-08-07
**Reviewer:** Agent (self-review) using code-review, essentialist, grill-me, mcda, pragmatic-semantics, pragmatic-cybernetics, idiomatic-rust skills
**Change scope:** W-1 through W-14 (dependency hygiene, module moves, dead code deletion, test strengthening)
**Diff base:** `831562dc24` (origin/main) → `5d3dc62986` (HEAD)
**Validation:** `cargo check --all-targets` (12 kask crates clean), `cargo clippy --all-targets -- --deny warnings` (9 crates clean), `cargo test --lib` (hkask-types 106, hkask-regulation 85, hkask-keystore 16, hkask-mcp-media 79 — all pass), 5 shell gates pass, diagnostics clean on all changed files

---

## Verdict: **Approve with fixes applied**

Zero Blockers remain after the review's own fix pass. Two Should-fix issues were found and fixed during the review. The change improves overall code health (dead code removed, dependency graph simplified, feedback loops restored, test pins strengthened).

---

## Findings

### Blocker → Fixed during review

#### B-1: F4 pin used `TypeId::of::<CONST>()` — does not compile (E0747)
**Force:** Prohibition (broken code — compile error)
**Evidence:** `crates/zed/src/main.rs:4224` — `let _ = std::any::TypeId::of::<hkask_regulation::DEFAULT_VARIETY_MAX_DEFICIT>();`. `DEFAULT_VARIETY_MAX_DEFICIT` is a `pub const f64`, not a type. Rust rejects const-as-type-parameter with E0747. Verified with `rustc` minimal reproduction.
**Root cause:** The test "passed" earlier because the bin target failed to compile before the test module was reached (pre-existing `open_curator_regulation_archive` error), so the test compilation error was never surfaced.
**Falsifier:** `rustc --edition 2021` on a minimal `TypeId::of::<MY_CONST>()` reproduces E0747.
**Fix applied:** Changed to `let _ = hkask_regulation::DEFAULT_VARIETY_MAX_DEFICIT;` (value reference pins the const's existence).
**Status:** ✅ Fixed.

### Should-fix → Fixed during review

#### S-1: F5 and F10 pins used `TypeId::of::<u32>()` / `TypeId::of::<bool>()` — trivially true, pin nothing
**Force:** Guardrail (advertised invariant without enforcement — `.rules` trap)
**Evidence:** `crates/zed/src/main.rs:4230` — `let _ = std::any::TypeId::of::<u32>();` claims to pin `SWARM_PANEL_CALL_CAP` but `u32` always exists; deleting the const doesn't break the test. Similarly L4238 — `TypeId::of::<bool>()` claims to pin `always_on` but `bool` always exists.
**Falsifier:** Delete `SWARM_PANEL_CALL_CAP` — test still compiles (the pin is theater).
**Fix applied:** F5 changed to `let _ = SWARM_PANEL_CALL_CAP;` (value reference). F10 changed to `TypeId::of::<kask_bridge::KaskCuratorSettings>()` (pins the struct that contains `always_on`; removing the field or struct breaks compilation).
**Status:** ✅ Fixed.

#### S-2: F25 pin used function pointer type — doesn't pin function name
**Force:** Guideline (weak pin — renames silently pass)
**Evidence:** `crates/zed/src/main.rs:4250` — `TypeId::of::<fn(Arc<McpRuntime>, ...)>()` pins the signature type but not the function name. Renaming `sync_kask_mcp_runtime_servers` doesn't break the test.
**Falsifier:** Rename the function — test still compiles.
**Fix applied:** Changed to `let _ = sync_kask_mcp_runtime_servers as fn(...);` (function item cast pins both name and signature).
**Status:** ✅ Fixed.

### Should-fix → Fixed during review

#### S-3: Stale "Moved to hkask-types" doc comments in moved loops files
**Force:** Evidence (Specification vs Implementation drift — pragmatic-semantics)
**Evidence:** `kask/crates/hkask-regulation/src/types/loops/actions.rs:3` — "Moved from hkask-regulation to hkask-types to break the circular dependency" — but the file moved **back** to `hkask-regulation`. Same in `core.rs:3`, `signals.rs:3`, `episodic.rs:3`. The `mod.rs` doc was updated correctly, but the individual files were not.
**Falsifier:** Read the files — the comments describe a move that was reversed.
**Fix applied:** Removed the stale "Moved" sentences from all 4 files, keeping the substantive doc content.
**Status:** ✅ Fixed.

### Nit / FYI

#### N-1: F23 pin still uses `TypeId::of::<fn(...)>()` (not fixed — original code)
**Force:** Guideline (same weak-pin pattern as S-2, but in the original code, not my change)
**Evidence:** `crates/zed/src/main.rs:4209` — `TypeId::of::<fn(&KaskSettings) -> Vec<...>>()` pins the signature but not the `kask_server_env` function name.
**Falsifier:** Rename `kask_server_env` — test still compiles.
**Status:** Not fixed (original code; out of this review's change scope).

#### N-2: `AGENT_SUBDIRS` lists dirs whose accessors were deleted
**Force:** Hypothesis (orphaned constant entries — the dirs are created but no code constructs paths to them)
**Evidence:** `kask/crates/hkask-types/src/agent_paths.rs` — `AGENT_SUBDIRS` lists `gallery`, `documents`, `library`, `sessions`, `portfolios`, `artifacts`. The accessor functions (`agent_gallery_dir`, etc.) were deleted as dead code, but the dirs are still created by `ensure_agent_dirs` / `identity.rs`. The dir names are created on disk but no code reads from or writes to them.
**Falsifier:** Check if any code uses `agent_dir(name).join("gallery")` etc. directly.
**Status:** Not acted on — the dirs may be needed by future code or external consumers. The constant + `ensure_agent_dirs` are live (used by `identity.rs`). The accessors were the dead part, not the dirs.

---

## Perspective rotations

### Essentialist (G1/G2/G3)

**G1 (Exist / Deletion test):**
- `keychain_keys` module: **Passes** — 21 keychain key constants in a single source of truth. Deleting it would force 21 bare string literals across 3 crates (complexity reappears). Behavior IS lost on deletion.
- `transcript` module: **Passes** — `TranscriptBundle` type with methods. Deleting it would force the media server to inline 179 lines of struct + method definitions.
- `loops` module: **Passes** — the loop type system (LoopId, Signal, Deviation, etc.) is consumed by `cybernetics_loop.rs` and the regulation policy. Deleting it would force 850+ lines of type definitions to reappear in `hkask-regulation`.
- 10 dead `agent_paths` accessors: **Correctly deleted** — deleting them caused no behavior loss (zero references).

**G2 (Surface / Interface count):**
- `keychain_keys`: 21 public items (all `pub const`) — exceeds the 7-item rule, but all are constants (the rule targets functions/types/traits). **Passes** with justification: a flat namespace of keychain key names is a single conceptual surface.
- `transcript`: 7 public items — **passes** exactly.
- `loops/mod.rs`: 11 `pub use` re-exports — exceeds 7, but these are re-exports of a type system (the rule targets direct definitions). **Passes** with justification.

**G3 (Contract / Abstraction trace):**
- No single-impl traits introduced. No pass-through wrappers. No unnecessary indirection. **Passes.**

### Grill-me (self-challenge)

**Q: Could the `allow(unused_crate_dependencies)` restoration on 4 lib roots be avoiding a real problem?**
A: The `allow` is on the lib root where `tokio`/`anyhow` is in `[dependencies]` for the bin target's `#[tokio::main]` / `anyhow::Result`. The lib genuinely doesn't use these deps in non-test code. Moving them to `[dev-dependencies]` was attempted and broke the bin (dev-deps aren't available to bins in regular `cargo build`). The `allow` with an accurate comment is the correct solution — the alternative (keeping `tokio` in `[dev-dependencies]` only) breaks the bin. **The restoration is correct.**

**Q: Is the `loops` move safe? Could a future upstream merge re-introduce the cycle?**
A: The cycle was between `hkask-regulation` and its subcrates (storage guard, SLO, seam watcher) — all deleted. A future upstream merge would need to re-add those subcrates AND have them depend on `hkask-types::loops`. The `loops` types are now in `hkask-regulation/src/types/loops/`, not `hkask-types`. If subcrates are re-added, they'd depend on `hkask-regulation` (which has the types), not `hkask-types` — no cycle. **The move is safe.**

**Q: Does the `transcript` move to `hkask-mcp-media` create a problem if another crate later needs `TranscriptBundle`?**
A: If a second consumer appears, `TranscriptBundle` would need to move back to `hkask-types` (or a shared crate). The ADR's "move when a second consumer materializes" rule applies. Currently, `hkask-mcp-media` is the sole consumer. **No problem now; watch for second consumers.**

### Pragmatic-semantics (IS/OUGHT)

- The `allow` comments now accurately describe IS (the lib doesn't use `tokio`) and OUGHT (the bin needs it). No drift.
- The `loops/mod.rs` doc now accurately describes IS (types live here) without the stale "moved to hkask-types" OUGHT. No drift.
- The `main.rs` test doc now accurately describes what's pinned (F2–F25 symbols reachable from a unit test) vs. the old "28 functional units" claim. No drift.

### Pragmatic-cybernetics (feedback loop analysis)

- The `unused_crate_dependencies` lint is now a **functional feedback loop** for 21 of 25 files (the `allow` was removed). For the 4 restored `allow` files, the loop is **intentionally broken** with an accurate comment — the lint would fire on a false positive (the dep IS used, just by the bin, not the lib). This is a documented degradation, not a silent one.
- The `check-unused-deps.sh` script (nightly lint) is still not wired into CI — this is a separate broken feedback loop (the script exists but doesn't run automatically). **Not addressed in this session.**

### MCDA (decision quality)

The key decisions evaluated against 5 criteria:

| Decision | Correctness | Simplicity | Reversibility | Feedback | Risk | Composite |
|----------|------------|-----------|--------------|----------|------|-----------|
| Remove `allow` from 21 files | 10 | 10 | 10 | 10 | 10 | 1.00 |
| Restore `allow` on 4 lib roots with comment | 9 | 8 | 10 | 8 | 9 | 0.88 |
| Move `loops` to `hkask-regulation` | 10 | 9 | 8 | 9 | 8 | 0.88 |
| Move `keychain_keys` to `hkask-keystore` | 10 | 9 | 9 | 9 | 9 | 0.92 |
| Move `transcript` to `hkask-mcp-media` | 10 | 9 | 8 | 9 | 8 | 0.88 |
| Delete 10 dead `agent_paths` accessors | 10 | 10 | 10 | 10 | 10 | 1.00 |
| Strengthen `kask_wiring_symbols_exist` test | 9 | 8 | 10 | 9 | 9 | 0.90 |
| Keep `template_type` in `hkask-types` (cycle) | 10 | 10 | 10 | 10 | 10 | 1.00 |

**Sensitivity:** The `allow` restoration decision is the most fragile — if the bin targets are ever refactored to not need `tokio` (e.g., using a different runtime), the `allow` becomes stale. The comment mitigates this by documenting the rationale.

### Idiomatic-rust (Hoare principles)

- **P1 (invalid states unrepresentable):** The `KaskCuratorSettings` pin makes the `always_on` field's existence unrepresentable-if-removed. Good.
- **P7 (errors as values):** No `unwrap()`/`expect()` introduced. The `let _ = CONST` pattern is safe (consts are infallible).
- **P8 (unsafe as contract):** No `unsafe` introduced. The `forbid(unsafe_code)` / `deny(unsafe_code)` attributes are preserved on all lib roots.

---

## Coverage honesty

**Checked:**
- All 25 `allow` removals/restorations (grep verified, clippy clean)
- All 3 module moves (`loops`, `keychain_keys`, `transcript`) — compile + test verified
- Dead code deletion (`agent_paths`) — test verified (106 pass)
- Stale `allow(dead_code)` removal (`regulation_policy`) — compile verified
- `main.rs` test strengthening — compile verified (F4, F5, F7, F10, F25 pins)
- Doc comment fixes (DIVERGENCE.md, check-hkask-no-zed-deps.sh, loops files)
- 5 shell gates (all pass)
- Diagnostics on 6 key files (all clean)

**Not checked:**
- Full `./script/clippy` (workspace-wide release clippy — blocked by concurrent build processes; targeted clippy on 9 affected crates passed)
- `cargo test` for `hkask-mcp-companies`, `hkask-mcp-corpus`, `hkask-mcp-condenser`, `hkask-mcp-curator`, `hkask-mcp-prediction-markets`, `hkask-mcp-scenarios` (pre-existing `hkask-storage` compile error from concurrent memory-refactor work prevents full test build)
- The `settings::init(cx)` move in `main.rs` (not my change — concurrent work)
- The curator ontology-axis recall code (not my change — concurrent work)
- The embedding-free memory fallback (not my change — concurrent work)

**Residual risk:**
- The 4 restored `allow` attributes could become stale if the bin targets are refactored. The comment mitigates this.
- The `AGENT_SUBDIRS` dirs are created but some may be unused (orphaned directory creation). Low risk — disk space only.
- The `check-unused-deps.sh` script is still not wired into CI. The `allow` removals are only verified by manual `RUSTFLAGS="--force-warn"` runs.

---

## Lessons learned

1. **`TypeId::of::<CONST>()` doesn't compile** — a const is not a type. Use `let _ = CONST;` to pin a const's existence. This was caught by the review's grill-me self-challenge, not by `cargo check` (because a pre-existing bin error masked the test compilation error).
2. **`TypeId::of::<u32>()` is always true** — pinning a primitive type pins nothing. Pin the actual symbol (value reference for consts, struct type for fields, function item cast for functions).
3. **`[dev-dependencies]` are not available to bin targets in regular `cargo build`** — only in test/benchmark builds. A dep needed by the bin's `#[tokio::main]` must stay in `[dependencies]`.
4. **Module moves must update doc comments in all moved files** — the `mod.rs` was updated but the individual files (`actions.rs`, `core.rs`, etc.) still had stale "moved to hkask-types" comments.

---

## Next review focus

The `check-unused-deps.sh` script should be wired into CI to close the feedback loop on the `allow` removals. The 4 restored `allow` attributes should be re-evaluated if the bin targets are refactored. The `AGENT_SUBDIRS` orphaned dirs should be investigated for whether the dirs are actually used by any code.
