# Continuation: Kask reliability program — Phase D + validation close-out

## Operator instruction and immediate objective

You are taking over an authorized implementation, mid-phase. The program
(two prior handoffs) completed and verified T01–T03, D01, and the storage
reliability addenda. The operator then ruled (2026-09-07) that three
"unwired" flags are **deceptions, not questions**: `kask.memory.memory_life_days`
must govern actual decay (T16), consolidation must really run (T17), and the
salience method-signals half must be wired (T18). A fourth item was added the
same day: **MCP servers must die with the zed-kask session** (T19) — they
orphan on app exit because nothing calls the runtime's stop path at quit.

The last session ended mid-validation when the machine became unusable (see
"Build discipline" below). Your first job: **validate or revert the
never-compiled changes** — a half-validated tree is the one state this program
must never sit in. Do not begin new tasks until the tree is green or reverted.

No commits/branches are authorized. The operator stages work via an external
process to track changes without committing — worktree/index divergence is
normal; never unstage or reset anything.

## Read first

1. `tasks/plan.md` — Phase D (T16–T18, operator ruling verbatim), all
   verification-evidence tables, the retraction record, and the follow-up queue.
2. `tasks/todo.md` — checkboxes with per-item evidence.
3. Project `.rules`; load the `program-manager` and `tdd` skills.
4. `kask/docs/architecture/core/magna-carta.md` — P1 user sovereignty / P4 clear
   boundaries; IS-vs-OUGHT discipline (claim only validated enforcement).
5. `DIVERGENCE.md` — D45 (kask MCP runtime load/unload + latch), D46 (build
   profiles), D49 (latest entry number; your new seams continue the sequence).
6. This prompt; then the current `git --no-pager --no-optional-locks status
   --short` and `git --no-pager --no-optional-locks diff` for the unstaged tail.

## Baseline

- HEAD: `2475305420ae065b5d1792c0f25cea471e558ae3` — unchanged since the
  program began. No commits, no branches.
- The operator's staging snapshots track changes without committing; the
  newest work may exist only as unstaged worktree deltas on top of the index.
- `target/` is ~33 GB with a mostly-built gpui debug tree from the interrupted
  bridge test builds — incremental resumption should be minutes, not tens of
  minutes. Do not clean it.
- sccache is installed and active (`~/.local/lib/kask-sccache/`).

## Verified green this program (do not redo; evidence in plan.md)

- T01 scenarios persistence (incl. publication/truncation crash-window test).
- D01 portfolio retention guard (5 recovery tests; companies suite 68/68).
- T02 + T02b redirect SSRF gates (raw-fetch validated client; discover client).
- T03 forgetting coverage (test-first RED observed; spec §6 updated).
- Storage-suite flake fixes: `min_idle(Some(0))` + 120s connection timeout +
  catalogue writer lock (10/10). Suite 60/60 twice at default parallelism.
- T16 curator-server side: `HKASK_MEMORY_LIFE_DAYS` read + parse (2 tests green)
  and hkask-memory decay behavioral test (1 green, exact R(t) formula asserted).

## Code-complete but NEVER COMPILED — validate or revert

**1. `kask/crates/kask_bridge/src/memory.rs` + `memory/curator_stores.rs` (T16 + T17):**
- T16: `RealMemoryPort::new` gained a `memory_life_days: f64` param (after
  `confidence_floor`); `CuratorStore::new` gained `db_path: String` +
  `memory_life_days`; `open_curator_store(db_path, passphrase, embedding_dim,
  memory_life_days)` is parameterized (was env-internal) and applies
  `.with_memory_life_days(...)`; the sensor fallback uses
  `MemoryStore::default_memory_life_days()` (was a literal `180.0`).
- T17 (operator-directed cleanup, not a patch): `start_consolidation_timer`
  rewritten interval-native — skip the immediate first tick (that IS the
  "wait one cadence" grace), every subsequent tick fires a pass; **deleted**:
  `cadence_should_fire`, the test-only `maybe_consolidate`, the
  `last_consolidation` field (production never maintained it — the timer kept a
  private copy), the `std::sync::Mutex` and `chrono::Utc` imports, four cadence
  unit tests (one pinned the no-op), and both old `maybe_consolidate` tests.
  The pre-refactor bug, for the record: `last=None` + `fire_when_no_last=false`
  meant the production timer NEVER fired — consolidation never ran in
  production (masked by the test-only entry using `true`).
- Tests in the tree, unrun: `consolidation_timer_fires_and_prunes_low_confidence`
  (paused tokio runtime — `start_paused = true`; wall-clock-bounded; RED-able
  against the old code), `consolidation_pass_prunes_low_confidence_and_keeps_the_rest`,
  `ingest_turn_does_not_fire_consolidation` (probe-survival form),
  `start_consolidation_timer_is_disabled_at_zero_cadence`,
  `open_curator_store_applies_configured_memory_life_days`
  (curator_stores.rs — needs the `tempfile` dev-dep that was added to
  kask_bridge's Cargo.toml), `mcp_env_emits_memory_life_days_only_when_configured`
  (mcp_env.rs), and the extended `curator_allowlist_matches_actual_reads`
  (mcp_servers.rs) asserting the `HKASK_MEMORY_LIFE_DAYS` allowlist entry.

**2. `crates/zed/src/main.rs` (upstream file, D-seam surface):**
- The `RealMemoryPort::new` call site passes
  `kask_settings.memory.memory_life_days` (one added argument).

**3. `kask/crates/hkask-mcp/src/runtime.rs` (T19 core, ours):**
- `McpRuntime::shutdown_all(&self) -> Vec<String>` implemented: union of
  `connections` + `launch_specs` + `cancellation_tokens` keys → `stop_server`
  each (idempotent); children were already spawned `kill_on_drop(true)`, so
  dropping the connection chain SIGKILLs them.
- The operator wrote (test-first, present in the tree, **unrun**):
  `shutdown_all_clears_every_reconnect_path` — seeds two specs/stamps, asserts
  both maps cleared. My implementation was corrected to satisfy it (union
  semantics, not live-connections-only).

**4. `kask/mcp-servers/hkask-mcp-curator/src/hkask_mcp_curator.rs` (T16):**
  validated green (2 tests) — but note `open_curator_stores` and `CuratorDb`
  gained the `memory_life_days` param threaded through `from_context` +
  `try_heal`; already compiled and passing.

## Pending work, in order

1. **Apply D50 — the build-profile fix** (root cause of the 12-hour stall;
   not yet applied): add to root `Cargo.toml` inside the existing
   `[profile.release.package]` section (it currently holds only
   `zed = { codegen-units = 16 }`):
   `"*" = { codegen-units = 16 }`
   Effect: external deps stop compiling at cgu=1 (multi-GB RSS each, pinned to
   one core — upstream Zed's profile applied to ~1500 dep crates; D46 fixed
   this for the MCP servers, D50 extends the same medicine to everything
   except the `zed` crate's own explicit setting). Thin LTO stays. Extend
   `kask/scripts/build/check-build-profile.sh` with a pin, add the DIVERGENCE
   entry (next free number), and record the operator's authorization in it.
2. **T19 remainder — wire the quit hook** (upstream `crates/zed/src/main.rs`,
   D-seam entry alongside D45's wiring): register `cx.on_app_quit` in the kask
   section near the `McpRuntime` construction; inside the quit future, run
   `runtime.shutdown_all()` on the tokio handle (`block_on` is acceptable at
   quit: the future is pure tokio, no GPUI re-entrance). **The returned
   `Subscription` must be kept alive** (a dropped Subscription unregisters —
   leak it or store it). Kill-chain evidence, for the entry: app exit never
   runs Rust destructors, so nothing ever dropped the transports; the
   `StdioTransport::Drop` kill chain (`crates/context_server/src/transport/
   stdio_transport.rs:229-251`) and `cmd.kill_on_drop(true)`
   (`hkask-mcp/src/runtime.rs` `start_connection`) existed all along — only the
   session-end call was missing. Pin via the `kask_wiring_symbols_exist`
   fn-pointer pattern used for D45. Add T19 to plan.md Phase D.
3. **The one validation build** (machine must be idle — see discipline below):
   `cargo test --offline --locked -p kask_bridge --lib -j 8` — covers every
   unrun test above plus the whole existing memory/consolidation suite (the
   T17 refactor touched shared fixtures; the full lib suite is the point).
   Then `cargo test --offline --locked -p hkask-mcp --lib -j 8` (shutdown_all
   test), and `cargo check --offline --locked -p zed -j 8` ONLY if the
   operator is not building zed themselves (their release build also compiles
   the main.rs change — coordinate instead of duplicating).
4. **RED evidence** per the session's established pattern (worktree-only
   `git show HEAD:path > path` reverts, index untouched, restore after): the
   paused-time timer test against the pre-refactor code (never fires →
   wall-bound assertion trips); the bridge store test with the
   `.with_memory_life_days` application removed (reports default 180).
5. **`./script/clippy`** for kask_bridge, hkask-mcp, hkask-mcp-curator.
6. **Docs**: plan.md evidence tables for T16/T17/T19; `kask/docs/reference/
   kask-settings.md` — the settings-flow chain comment (settings.rs:~587)
   lists six steps for a new knob; steps 1–5 are done for
   `HKASK_MEMORY_LIFE_DAYS`, step 6 (settings UI page) is deliberately
   consistent-skipped (no other kask.memory knob is surfaced there either —
   record that decision, don't silently leave a gap). Then continue T18
   (method signals at tag time, declared-method matching at compose time,
   keyword_overlap in episode ranking — full design in plan.md Phase D).

## Build discipline — the 12-hour lesson (operator-ruled)

- **Exactly one build at a time on this box.** The operator's builds
  (frequently `-j 16 --release -p zed`) always take priority. Before ANY cargo
  invocation: `ps -eo pid,etime,args | grep -E "rustc|cargo" | grep -v grep` —
  if anything is running, wait; never start a concurrent build, and never kill
  the operator's processes without an explicit instruction.
- The 12-hour stall anatomy: the operator's release build (cgu=1 multi-GB
  rustc per dep crate) + agent test builds concurrently → swap-thrash. The
  `-j 24` escalation ("use all the cores") was a mistake — on this tree,
  test-profile gpui builds peak GBs per crate; `-j 8` on an idle box is the
  validated ceiling. `-j 2` was safe but leaves the box mostly idle.
- sccache is active; `target/` holds the gpui debug tree — resume incremental.
- Trim your tool outputs (`head_lines`/`tail_lines`, no raw ps/grep floods):
  the editor hosting the session degrades with transcript size — the last
  session's editor main thread pegged at 125% CPU from rendering this
  conversation and typing lagged seconds. A fresh session + small outputs is
  the fix the operator already adopted once.

## Program state beyond this phase

- T18 (salience wiring) is the remaining Phase D task after validation.
- T04–T08 (Phase B/C) untouched; T09–T15 follow-up queue requires elaboration.
- The operator's standing directives: root-cause fixes, not patches; delete
  dead code only with evidence, flag designed-but-unwired capabilities;
  RED evidence for every behavioral claim; no conservative half-validated
  states. Validation commands and observed output or it did not happen.