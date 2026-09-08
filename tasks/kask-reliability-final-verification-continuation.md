# Continuation: finish the Kask reliability program and verify functionality

## Mandate and immediate priority

Continue the authorized reliability program in `tasks/plan.md` and `tasks/todo.md`. Phase D implementation and automated validation are complete; **the whole program is not complete**. Close the release/runtime verification gap, then finish the remaining scheduled tasks without bypassing their policy gates.

The operator's latest actions/instructions were: push the commit, rebuild the code, then prepare this continuation. **The commit is already pushed. The release rebuild has NOT started.** Do not mistake a successful `cargo check` for a rebuilt executable. No installation, editor restart, or operator-data backfill was performed or authorized by the rebuild instruction.

Read this prompt instead of treating the older `kask-phase-d-continuation-prompt.md` as current. That older prompt describes the formerly uncompiled tail and contains retracted claims.

## Read first

1. `tasks/plan.md`: current close-out/evidence tables, Phase D corrections, T04–T08 acceptance criteria, T09–T15 follow-up queue, risks and checkpoints.
2. `tasks/todo.md`: checked activities and outstanding operator checkpoints.
3. `.rules`; load `program-manager` and `tdd`. Read the relevant crate README before editing.
4. `kask/docs/architecture/core/magna-carta.md`: P1 user sovereignty, P4 boundaries, and IS versus OUGHT. Name the constraining invariant before the first edit.
5. `DIVERGENCE.md`: D45 runtime load/unload/latch, D46 split build profiles, D50 external-dependency codegen, D51 shutdown, D52 Clippy default.
6. For memory/corpus work: `kask/docs/architecture/memory-system-specification.md`, `kask/docs/reference/kask-settings.md`, and `kask/docs/reference/mcp-servers/corpus.md`.

## Git and workspace state

- Root: `/home/mdz-axolotl/Clones/zed-kask`; branch: `main`.
- Last verified HEAD: `2aef59d09b715633d1bf80756f7cefda6eab38c9`.
- Remote: `origin` = `https://github.com/mdz-axo/zed-kask.git`.
- `git --no-pager ls-remote --exit-code origin refs/heads/main` returned that exact hash. Local `main` and `origin/main` matched. Do not create an empty or duplicate commit/push.
- The tree was clean before writing this handoff. This new file and its plan/checklist links are subsequent documentation changes.
- An external process stages AND commits/pushes during sessions. Worktree/index divergence is normal. Never unstage, reset, rewrite history, or revert another process's changes. No further commit/branch/push action is needed unless the operator requests it.
- Original program baseline: `2475305420ae065b5d1792c0f25cea471e558ae3`. Current HEAD already contains repairs; it is NOT a valid pre-fix mutation baseline.
- No compiler jobs were running at the last check. Recheck rather than assuming this remains true.

Start with status, recent log, unstaged/cached diff summaries, and process inspection. Resolve unexpected changes before editing.

## Verified work — do not redo indiscriminately

Earlier evidence in plan.md closes T01 scenarios persistence, D01 shared-DB research retention, T02/T02b redirect SSRF protection, T03 distillation-gated forgetting, and storage parallel-flake repairs. Do not repeat those suites unless a new change affects them.

Latest observed Phase D results:

| Command | Result |
|---|---|
| `cargo test --offline --locked -p hkask-mcp-corpus -p hkask-memory --lib -j 8` | 85 corpus + 32 memory tests passed; corpus lib includes real public-tool retrieval tests |
| `cargo test --offline --locked -p kask_bridge --lib -j 8` | 182 passed, including new overlap-ranking regression |
| `cargo test --offline --locked -p hkask-types --lib -j 8` | 33 passed |
| `cargo test --offline --locked -p hkask-mcp --features test-fixture -j 8 -- --test-threads=1` | 17 unit + 14 real-child-process integration tests passed |
| `HKASK_BUILD_JOBS=8 CARGO_NET_OFFLINE=true ./script/clippy --locked -p hkask-types -p hkask-memory -p kask_bridge -p hkask-mcp -p hkask-mcp-curator -p hkask-mcp-corpus` | Passed, including all targets/features, warnings-as-errors, machete, typos, buf; final observed wrapper time 73s |
| `cargo check --offline --locked -p zed -j 8` after T18 | Passed in 31.53s |
| Build-profile pin, MCP test-presence ratchet, targeted rustfmt, typos, diff checks | Passed; ratchet 0 violations/0 gaps is presence evidence only |

T16 curator parser/store and decay-formula tests were green in the incoming handoff; full provenance is in plan.md. Some `/tmp` logs vanished during this long session; do not invent their availability. Verified surviving logs at handoff: `target/kask-t18-bridge-tests.log`, `target/kask-t18-types-tests.log`, `target/kask-t18-zed-check.log`. The durable evidence tables preserve observed earlier results.

## What Phase D actually changed

### T16: configured memory lifetime

`RealMemoryPort::new` threads memory lifetime into `CuratorStore` and reopen. `MemoryStore::with_memory_life_days` now governs actual bridge decay. Non-default settings emit `HKASK_MEMORY_LIFE_DAYS`, scoped to the curator server, which validates and applies it on both store-construction paths. Sensors read the applied store; unavailable-store readings remain default estimates, not measured behavior.

Historical RED observed 180 instead of configured 30 without the application; GREEN afterward. These are constructor/reopen settings, **not newly implemented live bridge reconfiguration**.

**Handoff correction:** Settings → Kask → Memory already exposes Memory Life (`crates/settings_ui/src/pages/kask_page/memory.rs`). It is not a skipped sixth wiring step. Do not add duplicate UI or record the old proposed “consistent skip.”

### T17: real consolidation scheduling

The original production timer had `last=None` plus `fire_when_no_last=false`, so it never fired. The first retraction proved callers existed, NOT that scheduling worked. Callback-only tests masked this.

The interval-native timer skips its immediate tick, then fires at `max(configured cadence, 60s)`; zero disables it. Old timestamp/flag/test-only machinery is gone. Removed the obsolete one-hour polling cap, which otherwise shortened a two-hour setting. Historical no-fire RED and two-hour RED were observed; full suite GREEN. Do not restore either bug from old comments/history.

### T18: end-to-end method signals and keyword ranking

- `MethodSignals` is a typed record in `hkask-types::corpus`, re-exported by `hkask-memory::salience`; extraction remains in memory (no dependency cycle).
- Tagging writes `ontology.method_signals` and `how`, even on malformed/spoofed LLM output. Consolidation recomputes measurements after synthesis.
- `PassageIndex::publish_durable` upserts current `text` and `method_signals` h_mems, attributed to the server writer. Without this persistence, compose would still have no source to read. Replacement does not accumulate stale rows. Existing partial-write error semantics remain; no cross-record transactional guarantee was added.
- Existing cognition YAML accepts `embedding.retrieval.declared_method`, with `name` and per-field thresholds under `signal`. All configured thresholds must match. Omitted declaration preserves selection. Missing metrics exclude/count candidates (`method_signals_missing` in compose/rewrite output); corrupt metadata/read failures error. Legacy scalar `declared_method.threshold` is explicitly rejected, not silently ignored.
- Recovered `keyword_overlap_score` from `2b651fc956` (deleted later in `73554617ce`). Keyword relevance is `0.5 × matches/query entries`; semantic scores, query-word selection, confidence/connectedness weighting, and dedup precedence stay intact.
- Wiring exposed short-passage passive-voice `0/0` → NaN → JSON null. Minimum denominator one plus a round-trip test fixes it.

RED was observed for missing tagging metadata, missing durable metrics, ignored composition filtering, invalid short-passage serialization, and wrong recall order. GREEN controls cover replacement, consolidation recomputation, unconfigured filtering, missing/corrupt metadata, and unsupported shorthand.

**Compatibility:** legacy JSONL still deserializes. Old corpus DBs need re-embedding to populate method metadata; do not auto-backfill operator data. `corpus_embed` recomputes it without an extra generation call, but normal embedding costs apply.

### T19: owned process termination, not just map cleanup

Duplicate `shutdown_all` definitions caused the operator's E0592 build failure. Removing the duplicate was insufficient: a real-process test showed rmcp's detached/graceful cleanup still leaked a child.

`McpRuntime` now owns every child before handshake, gives rmcp only pipes, and awaits cancellable kill-and-reap tasks during stop/shutdown. Lifecycle locking protects publication; terminal shutdown rejects queued new starts. `wire_kask_mcp_shutdown` in `crates/zed/src/main.rs` registers `on_app_quit`, uses the Tokio handle, and `.detach()` retains its subscription.

Connected and discovery-blocked children, repeated shutdown, and rejected late starts are tested. Existing reconnect/stop/replacement tests pass. The production hook compiles; **the fn-pointer unit pin was not separately executed, and a live rebuilt-editor quit has NOT been witnessed**. Upstream `StdioTransport::Drop` is a different transport; do not cite it as proof of Kask child termination.

## Next: rebuild, then verify the actual application

1. Verify pushed revision and absence of active builds. Preserve `target/`; do not clean.
2. Run build-only commands matching `kask/scripts/build/install.sh`: editor on `release`, servers on `release-mcp`. Read `mcp-servers.txt` first and use its current list. Verify/wire the existing sccache wrapper. Do not run the full install script just to build; it has installation/dependency side effects.

```sh
cargo build --offline --locked --release --package zed --jobs 8
```

After completion and another process check:

```sh
cargo build --offline --locked --profile release-mcp --jobs 8 --package hkask-mcp-research --package hkask-mcp-companies --package hkask-mcp-corpus --package hkask-mcp-training --package hkask-mcp-kata-kanban --package hkask-mcp-curator --package hkask-mcp-portfolio --package hkask-mcp-scenarios --package hkask-mcp-prediction-markets --package hkask-mcp-swarm --package hkask-mcp-media
```

3. Record command, revision, profile, exit status, and elapsed time. Existing executable files alone are not rebuild proof. Outputs are under `target/release/` and `target/release-mcp/`; do not install them without authorization.
4. Coordinate launch/quit with the operator: the current editor may host this agent. Never close it autonomously. Verify the tested process actually runs the rebuilt executable, not an already-running older instance; inspect supported launch/isolation options rather than invent flags.
5. Before normal app quit, record the managed children/PIDs and executable paths. Observe their termination afterward from an independent bounded watcher or operator terminal. Include a non-responsive/pending-start case through the isolated fixture when needed. Do not blanket-kill unrelated MCP services and call that a shutdown pass.
6. Close Checkpoint D only with recorded operator review/live result. Also resolve outstanding Checkpoint A review before advancing through its gated phases. If functionality fails, reproduce at the real seam, write RED, fix, then rerun affected tests/checks. Do not paper over the failure in docs.

## Remaining program: T04–T15

T04–T08 are untouched. Use their full acceptance criteria in plan.md, not these shorthand reminders:

- **T04:** active-at-scan work must later distill without a new turn/restart; transient inference failure retries. Preserve watermark-before-lesson ordering and bounded startup discovery. Test successive production-cursor passes, not an epoch-wide helper scan.
- **T05:** reserve session authorization before external dispatch, including independent instances sharing SQLite. Recover/ratify ambiguous-outcome settlement policy first; never refund uncertain external acceptance automatically or conflate this contract with deprecated per-call budgets.
- **T06:** post-spawn bookkeeping failure must retain durable replay protection. Test actual successful spawn count, concurrent retry, reopen, and clean pre-effect rejection.
- **T07:** Applied acknowledgments require actual supported effects; failed, unsupported, and dampened are distinct. No new autonomous actuator/authority.
- **T08:** explicit domain/severity/evidence reaches the real human-review queue/channel. Test delivery/persistence failures separately; void best-effort sinks do not prove durable success. Depends on T07.

Run cumulative checks and obtain operator review at Checkpoints B/C. Do not silently choose unresolved retention, settlement, acknowledgment-schema, or escalation-routing policies.

T09–T15 require task elaboration and operator scheduling: passage deletion ownership, exact harness comparison, market identity calibration, cap-reset evidence, retrain finalization, IPC discovery, skill-feedback sensing. Recover specs, define falsifiable acceptance/failure controls, price unresolved risks, then schedule small slices. Their existence in the queue is NOT permission to delete intended capabilities or claim the program complete.

## Build/execution discipline

- Exactly one build at a time. Before EVERY Cargo invocation:
  `ps -eo pid,etime,args | grep -E 'rustc|cargo|clippy-driver' | grep -v grep`.
  Operator builds have priority; wait rather than queue on the lock. Do not kill their processes without explicit instruction.
- Use eight jobs; never escalate to 16/24 to “make it faster.” No `cargo clean`, cache deletion, lock deletion, or gratuitous profile changes.
- D50's `"*"` codegen override affects EXTERNAL dependencies only; workspace crates may still use cgu=1. Thin LTO remains. D46's MCP profile is separate.
- D52 makes `./script/clippy` dev-profile by default. Explicit profiles still work. Preserve all-target/all-feature/warnings coverage; use the wrapper, not bare Clippy.
- Warm test artifacts do not imply warm check/release/Clippy artifacts. The prior cold graphs took tens of minutes. The final warm lint/check took 73s/31.53s; neither is a guaranteed rebuild duration.
- sccache at `~/.local/lib/kask-sccache/` is installed, not magic: inspect stats/config rather than assuming hits or a cache size. Do not reset stats/cache casually.
- Use bounded commands (60 minutes per validation/build was approved), durable logs under `target/`, and small periodic progress summaries. After timeout, report progress and obtain the next window; do not repeatedly restart blindly. No giant rustc/ps/log dumps: editor responsiveness degraded badly under transcript load.
- One agent edits the tree. No half-refactors; no new task before the current slice compiles/tests. Use scratch fixtures, not live databases/providers. Preserve external staging/history.

## Completion standard

Every accepted task needs spec provenance, observed behavioral RED, GREEN through real producer/storage/consumer paths, failure controls, affected suites/checks/lints, residue review, and aligned docs. State exactly what ran. Build success is not functional success; test counts are not operator confirmation.

Keep plan.md/todo.md current. Finish with a closure ledger: verified tasks, operator decisions, scheduled remaining tasks, exact evidence, and remaining live checks. Do not label “Phase D automated checks complete” as “the reliability program complete.”
