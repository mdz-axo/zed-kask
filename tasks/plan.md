---
title: "Kask regression-first reliability plan"
creator: "Zed coding agent"
date: "2026-09-07"
type: "bibo:Document"
status: "Checkpoint A ratified 2026-09-08; Phase B (T04) in progress; live quit observation pending"
baseline: "2475305420ae065b5d1792c0f25cea471e558ae3"
---

# Kask regression-first reliability plan

## Target condition

Authorization history: the operator first requested this regression-first plan, then on 2026-09-07 said **"D01 yes. please proceed"**, ratifying research retention and authorizing implementation. The latest instruction resumes the Phase D plan after ratifying the development-profile Clippy wrapper. The T16/T17/T19 validation gate was closed before implementing T18; Phase D implementation and automated checks are now complete. No commits, branches, unrelated policy changes, or acceptance of deferred risks were authorized.

The target is to prevent the identified loss of recoverable data, private-network boundary bypass, over-authorized external dispatch, duplicate agent creation, and false completion evidence. Fix one observable failure path at a time; deepen existing modules only after behavioral tests pass.

Audit findings are source-backed hypotheses with concrete triggers, not reproduced incidents. **State 2026-09-09: T01–T03 (+T02b), D01, T04–T06, T07–T08, T16–T19 (automated), and the follow-up queue T09–T14 are ALL verified; the Phase E teardown (C1–C5) is complete. Remaining: T15's operator_feedback producer (gated on the operator's routing decision), the live application-quit smoke test, and Checkpoint D operator review.** Owner for technical closure is the receiving coding agent; unresolved policy decisions remain with the operator. Later-wave tasks remain tracked, not silently abandoned or declared safe.

## Current validation close-out — 2026-09-08

**Current continuation:** [Complexity teardown and program resumption](kask-teardown-continuation-prompt.md) (Phase E). The prior [final-verification continuation](kask-reliability-final-verification-continuation.md) is historical: its release-build gap has since closed (evidence below), Checkpoint A is ratified, T04–T06 are verified, and the program is now in the operator-ruled teardown mode. Live editor-quit confirmation and Checkpoints B/D remain pending. The reliability program is **not complete**.

### Release build and runtime identity — 2026-09-08

**Scope:** execute the operator-authorized build-only commands, verify artifact/runtime identity, and update this plan/checklist. Magna Carta P1/P4 prohibit expanding that authorization into installation, restart, data backfill, or policy decisions. IS/OUGHT prohibits treating a successful build or matching binary identity as observed shutdown. No production source/configuration was edited and no installation, launch, quit, backfill, commit, staging, reset, or push was performed by this continuation agent.

**Revision/preflight:** local `main`, `origin/main`, and `git ls-remote --exit-code origin refs/heads/main` matched `2aef59d09b715633d1bf80756f7cefda6eab38c9`. HEAD remained unchanged across both builds. Compiler-process checks were empty before each Cargo invocation and after completion. Existing task handoff changes and unrelated research documentation changes were preserved; the index was untouched. The current `kask/scripts/build/mcp-servers.txt` contains the 11 packages listed below.

Both commands used `RUSTC_WRAPPER=/home/mdz-axolotl/.local/lib/kask-sccache/sccache`, eight jobs, offline/locked resolution, and independent 3600-second limits. The inspected cache reported 9 GiB used / 10 GiB maximum, 4 Rust hits / 578 Rust misses before these builds; no cache/configuration/statistics reset was performed.

| Command / observation | Observed result |
|---|---|
| `cargo build --offline --locked --release --package zed --jobs 8` | **Exit 0**, wrapper elapsed **1.01s**, Cargo **0.87s**; started `2026-09-08T22:29:42Z`. Output is `target/release/zed-kask` (package `zed`, binary `zed-kask`). Cached linker output reports 62 WebRTC/OpenSSL symbol-type mismatches; retained in the log, not claimed fixed. |
| `cargo build --offline --locked --profile release-mcp --jobs 8 --package hkask-mcp-research --package hkask-mcp-companies --package hkask-mcp-corpus --package hkask-mcp-training --package hkask-mcp-kata-kanban --package hkask-mcp-curator --package hkask-mcp-portfolio --package hkask-mcp-scenarios --package hkask-mcp-prediction-markets --package hkask-mcp-swarm --package hkask-mcp-media` | **Exit 0**, wrapper elapsed **0.71s**, Cargo **0.60s**; started `2026-09-08T22:30:20Z`. All 11 executable artifacts present under `target/release-mcp/`. |
| Artifact freshness | Cargo reused already-current artifacts, not a fresh compile/relink. Observed artifact mtimes precede these commands: editor `22:17:29Z`, servers `22:18:59Z`–`22:20:04Z`. Build completion is supported by actual Cargo exits, not existence alone; this continuation does not claim to have produced those earlier artifacts. |
| Live executable identity via `/proc/<pid>/exe` | Editor PID **90198**, path `/home/mdz-axolotl/.local/bin/zed-kask`; 11 directly parented managed children, also under `.local/bin`. All 12 ELF build IDs match the corresponding verified release artifacts. Editor build ID: `5274b415107243332ebf12a1f3068694d0928f49`. Whole-file SHA-256 differs for all 12; identity evidence is explicitly ELF build-ID matching, not byte equality. |
| Live shutdown | **Not observed.** An independent read-only watcher is now armed (see the live-runtime subsection below); no process was stopped. |
| Live memory-life emission (T16) | Running curator child PID 92642 carries `HKASK_MEMORY_LIFE_DAYS=30` in its environment — the operator's `kask.memory.memory_life_days: 30.0`, emitted by the rebuilt editor through the T16 env path. |
| Live memory-life decay (T16) | `curator_memory_recall` of `skill_use_issue:program-manager` returned all 11 h_mems at confidence `0.4998709657267828` = 0.5·exp(−Δt/30) with Δt ≈ 669 s since the previous recall touch — the running curator applies the configured S=30 decay at recall time. |

**Durable logs:** `target/kask-reliability-rebuild-20260908T222941Z/{editor,servers}.log`; companion `{editor,servers}.json` record exact commands, revision, timestamps, wrapper, exit status and elapsed time. `live-executable-identities.json` records the 12 PID/parent/start-time identities, executable paths, SHA-256 values and ELF build IDs. Existing earlier test evidence below was not indiscriminately rerun; no new behavioral RED/GREEN is claimed for this evidence-only slice.

**Next operator-controlled steps:** confirm Checkpoint A review; the independent bounded read-only watcher is armed and identities were refreshed at arm time (editor 90198 + 11 children, PID + start ticks). Let any in-flight editor work finish (a parallel agent session was still running tests at arming), then let the operator normally quit the build-ID-matched editor. The watcher records child termination/reaping and any survivors without blanket-killing services. Confirm normal quit with the operator (process disappearance alone does not establish how it exited). The isolated pending-start/nonresponsive fixture evidence remains in the T19 table; it does not replace a live application result. Source inspection confirms `--user-data-dir` exists, but no isolated app was launched or complete Kask-data/keychain isolation claimed.

**Closure ledger:** release-build verification — fixed/verified, coding agent; executable identity — verified by ELF build IDs, coding agent; live T16 emission/decay — observed on the running build, coding agent; quit watcher — armed and detached, coding agent; Checkpoint D live quit/review — awaiting operator coordination, coding agent observes and operator confirms; Checkpoint A review — operator decision, blocks Phase B; T04–T08 — coding agent queue after review and task-specific policy gates; T09–T15 — coding agent elaboration plus operator scheduling, not execution-authorized. Deferral risks remain in the risk register; no acceptance of those risks is inferred.

### Checkpoint A — operator review — RATIFIED 2026-09-08

The operator reviewed the Phase A evidence in a Q&A session and **ratified all recommendations** (verbatim instruction: "ratifying all recommendations - please proceed"):

- **Q1/T01 accepted** with its one documented boundary (prior-snapshot-survives-failed-replacement is design-reviewed, not fixture-pinnable).
- **Q2/T02 boundaries accepted as scope, not gaps:** provider-side fetches out of gate; multi-hop permitted chains pinned by decision tests; proxy env ignored with a build-time warn.
- **Q3/T02b permissive RSS policy re-confirmed:** `rss_subscribe`/`rss_fetch`/`rss_synthesize` keep the permissive redirect policy (user-curated feeds); only `discover` is strict.
- **Q4/T03 conservative defaults ratified:** ambiguous duplicate passages keep their embedding (bounded duplicate leak over erasing uncovered recall); unparseable-watermark threads are skipped (no proof, no deletion).
- **Q5/D01 trade-off confirmed:** incompatible mixed databases error at startup rather than reset.
- **Q6/Checkpoint A closed; Phase B entry approved:** proceed T04→T05→T06 as scheduled; T05's ambiguous-outcome settlement policy is deferred to T05's start, with T04 first.

Checkpoint A is closed. Phase B (T04–T08) is unblocked; T04 begins immediately.

### Live runtime behavior and quit watcher — 2026-09-08

**Live T16 emission and decay (observed against the running build):** the running curator child (PID 92642) carries `HKASK_MEMORY_LIFE_DAYS=30` in its environment — the operator's `kask.memory.memory_life_days: 30.0` setting, emitted by the rebuilt editor through the T16 env path. A `curator_memory_recall` of `skill_use_issue:program-manager` returned all 11 h_mems at confidence `0.4998709657267828`, exactly `0.5 × exp(−Δt/30)` with Δt ≈ 669 s since the previous recall touch (a recall touches `recalled_at` per the store's read semantics — the observation method itself resets the clock) — the running curator applies the configured S=30 Wozniak-Gorzelanczyk decay at recall time. The prior session's `decay-live.json` (0.4999558275686026) is consistent with the same S=30 (Δt ≈ 229 s since a preceding touch), not with decay-since-creation and not with a divergent constant; it is live decay evidence. This is production observation of the wired behavior supplementing the unit-test GREEN evidence; it does not replace the Checkpoint D review.

**Quit watcher armed:** `target/kask-quit-watch-20260908T225830Z/` — a detached (parent `systemd --user`, own session), read-only, bounded (7200 s from 2026-09-08T23:02:16Z) observer tracking editor PID 90198 and its 11 managed children by PID + start ticks (PID-reuse safe; arm-time identities match `live-executable-identities.json`). It logs termination/reparenting transitions to `watcher.log` and writes `summary.txt` with a CLEAN/LEAK/EXPIRED result when every tracked process terminates or the deadline passes; it never signals or kills anything. Interpretation caveats are in its `README.txt`: disappearance does not establish how the editor exited (the operator confirms the manner), and 2 s poll granularity limits ordering within a single poll. If it expires unused, re-arm per the README using the then-current editor PID. The watcher's detection paths were validated against scratch processes before relying on it (`target/kask-quit-watch-selftest/`): child termination while the parent lived, parent termination, reparenting of a surviving child (old→new PPID), `ALL_TRACKED_TERMINATED`, and the LEAK verdict all fired correctly; the real editor and its 11 children were untouched throughout, and the scratch fixture was removed afterward.

The operator authorized continuation, then D52's development-profile Clippy default. The original uncompiled tail and T18 are now compiled and tested; **the full Zed check and scoped lint passed** in the operator-approved incremental window. T18 began only after that gate closed. Live editor-quit confirmation remains an operator checkpoint, not a claimed automated result. No commits, branches, staging, unstaging, or resets were performed by this agent. An external process advanced HEAD through `e102d515e8`, `eadb5b9521`, and `5fd4bac424`; the latest observed tree was clean before this documentation update. Do not use current HEAD as the pre-fix behavioral baseline: use the original `2475305420ae065b5d1792c0f25cea471e558ae3`.

### Verification evidence — T16/T17/T19 and build gates

| Command / experiment | Observed result |
|---|---|
| Operator's release build | E0592: duplicate `shutdown_all` definitions. Reconciled by retaining the lifecycle-locked implementation, not the appended union/loop duplicate. |
| `cargo test --offline --locked -p kask_bridge --lib -j 8` | Initial compile found missing Tokio `test-util` and a twice-moved test record. After those fixes: 179 passed, zero-cadence fixture failed without Tokio. Corrected its annotation; 180/180 passed. Final suite after long-cadence regression: **181 passed, 0 failed** (build 1m39s, tests 26.83s). |
| Historical RED: `cargo test --offline --locked -p kask_bridge --lib memory:: -j 8` | Re-applied the original timer + cadence helper from `2475305420`, initializing the deleted timestamp field's cold state to `None`; removed only `.with_memory_life_days(...)` in the bridge store. **51 passed, 3 failed**: timer never fires (wall-clock assertion), no pass at two hours, store reports **180 instead of 30**. Both worktree files restored in `finally`; index untouched; full GREEN suite above ran after restoration. |
| Test-first `consolidation_timer_respects_cadences_longer_than_one_hour` | RED: two-hour setting pruned at one hour (0 vs 1). Changed obsolete `clamp(60, 3600)` to `max(60)`; GREEN in final suite. |
| `cargo test --offline --locked -p hkask-mcp --lib -j 8` | **17/17 passed**, including operator's `shutdown_all_clears_every_reconnect_path`. |
| `cargo test --offline --locked -p hkask-mcp --features test-fixture --test reconnect_integration shutdown_all_kills_live_and_pending_children -j 8 -- --test-threads=1` | Test-first RED: existing shutdown leaked a fixture child despite cleared maps (test killed its own leaked PID). Root cause: rmcp detached/graceful cleanup, not awaited kill-and-reap. Runtime now owns children independently of rmcp pipes and awaits termination. Strengthened RED: queued start after shutdown returned `Ok(())`; terminal shutdown latch now rejects it. |
| `cargo test --offline --locked -p hkask-mcp --features test-fixture -j 8 -- --test-threads=1` | Final **17 unit + 14 real-process integration tests passed**, 0 failed; build 47.61s, unit 1.68s, integration 8.72s. Existing stop/replacement/reconnect/discovery regressions remain green. |
| `cargo check --offline --locked -p zed -j 8` | **Timed out at 30 minutes** during dependency compilation/checking (last recorded unit `cosmic-text`); no completed Zed check. |
| `HKASK_BUILD_JOBS=8 CARGO_NET_OFFLINE=true ./script/clippy --locked -p kask_bridge -p hkask-mcp -p hkask-mcp-curator` before D52 | **Operator-interrupted**, not passed: forced release/all-target/all-feature graph logged 266 compilation + 657 check entries, ending at `agent`. |
| Same scoped Clippy command after D52 | **Timed out at 20 minutes**, not passed: dev graph logged 17 compilation + 207 check entries, reaching `workspace`/`agent_servers`. Progress was sampled every minute. No compiler jobs remained afterward; current swap usage was zero. No automatic retry authorized by a timeout. |
| Operator-approved resume: same scoped development-profile Clippy command | **Passed**: compilation/checking 7m04s, wrapper 447s, `--all-targets --all-features --deny warnings`; machete/typos/buf clean. |
| Operator-approved resume: `cargo check --offline --locked -p zed -j 8` | **Passed**, 43m06s to finish the remaining dependency graph before T18. |
| `bash kask/scripts/build/check-build-profile.sh` | D50 RED without external-dependency override, GREEN with it. D52 stub-Cargo RED with forced `--release`, GREEN after removal; exact flags, explicit profiles, workspace default, and failure propagation pinned without a compiler. |
| `bash -n script/clippy kask/scripts/build/check-build-profile.sh`; `git diff --check` | Passed for the build-wrapper slice; documentation close-out diff check also passed. |
| `bash kask/scripts/check-mcp-tool-tests.sh` | **0 violations, 0 allowlisted gaps**; presence ratchet only, not behavioral proof. |
| `rustfmt --edition 2024 --config skip_children=true --check` on runtime.rs, reconnect_integration.rs, memory.rs, memory/curator_stores.rs | Passed. |

Some earlier logs were ephemeral under `/tmp/kask-phase-d-*.log` and are no longer available; this table records the outputs observed in-session. Final bridge/types/app-check logs are under `target/kask-t18-*.log`. The T16 curator-server parser/store tests and hkask-memory formula test were already green in the incoming handoff; not rerun here.

**Closure:** automated checks passed; the operator owns Checkpoint D review and live editor-quit confirmation. T04–T15 remain in their existing queue and are not executed by this close-out. The MCP presence gate and targeted Rust formatting checks pass. No pending task is marked complete solely on compilation or a symbol pin. D50/D52 behavior is verified at the script seam; no end-to-end build speedup is claimed. The existing Settings → Kask → Memory page already exposes `memory_life_days` (`crates/settings_ui/src/pages/kask_page/memory.rs:73–94`, linked by `kask_page.rs`): the handoff's proposed step-6 "consistent skip" was factually incorrect and is not adopted.

### T18 implementation and verification — 2026-09-08

**Delivered:** typed `MethodSignals` lives in `hkask-types::corpus` and is re-exported from `hkask-memory::salience` (no reverse dependency). Tagging stores computed `ontology.method_signals` even on LLM fallback, and consolidation recomputes it after synthesis. `PassageIndex::publish_durable` persists/replaces current passage text and signals under the same entity, with the server's writer identity and ontology provenance; embedding/consolidation callers carry that identity. This additional persistence step was necessary: composition reads h_mems, not tagged JSONL, so tag-time metadata alone would still be unwired.

The existing cognition YAML accepts `embedding.retrieval.declared_method`; all configured `signal` thresholds are checked by `DeclaredMethod::matches`. No declaration preserves selection; missing signals are counted in `method_signals_missing`, corrupt signals/read failures error, and the unimplemented legacy scalar `threshold` is explicitly rejected. Existing DBs need `corpus_embed` backfill to gain the new metadata; no operator corpus was modified. Metadata/vector publication retains the existing explicit partial-failure contract, not a new transactional guarantee.

Recovered `keyword_overlap_score` from `2b651fc956` (later deleted in `73554617ce`) and wired it into keyword episode relevance as `0.5 × matches / query entries`. Semantic scoring, query-word selection, connectedness/confidence weighting, and dedup precedence remain unchanged. Wiring also exposed short-passage `0/0` in passive-voice estimation; the denominator now has a minimum of one and a JSON round-trip regression pins finite output.

| Command / experiment | Observed result |
|---|---|
| `cargo test --offline --locked -p hkask-mcp-corpus --lib tagging_persists_method_signals_without_trusting_the_model -j 8` | Initial fixture lacked classifier configuration; corrected with an isolated test process and offline inference. Behavioral RED: `ontology.method_signals` was Null vs measured object. GREEN after wiring; valid/spoofed and malformed LLM output both covered, one LLM call per tag batch. |
| Same corpus command filtered to `durable_embeddings_keep_current_method_signals` | RED: no persisted signals. GREEN: signals/text survive reopen and replacement, correct writer identity, exactly two current metadata rows per passage. |
| Same corpus command filtered to `composition_filters_durable_passages_by_declared_method` | RED: two exemplars selected instead of one. Exposed the short-passage NaN on first wired run; fixed at extraction. Final GREEN covers matching/unconstrained requests, corrupt/missing metrics and unsupported shorthand; missing metrics are surfaced, not just empty-success asserted. |
| `cargo test --offline --locked -p hkask-memory --lib method_signals_round_trip_for_short_passages -j 8` | RED: null could not deserialize as f32. Fixed the zero estimated-verb denominator; GREEN in the full suite. |
| `cargo test --offline --locked -p kask_bridge --lib recall_ranks_keyword_episodes_by_overlap -j 8` | RED: weak one-keyword episode ranked above stronger overlap. GREEN after normalized scorer wiring. |
| `cargo test --offline --locked -p hkask-mcp-corpus -p hkask-memory --lib -j 8` | **85 corpus + 32 memory tests passed**, 0 failed. Corpus lib suite includes public-tool retrieval tests and consolidation metric recomputation; no separate tool-behavior target exists in this crate. |
| `cargo test --offline --locked -p kask_bridge --lib -j 8` | Final observed run: **182 passed**, 0 failed, including the new ranking test. Re-ran when the earlier temporary log was unavailable; retained under `target/`. |
| `cargo test --offline --locked -p hkask-types --lib -j 8` | **33 passed**, 0 failed. |
| `HKASK_BUILD_JOBS=8 CARGO_NET_OFFLINE=true ./script/clippy --locked -p hkask-types -p hkask-memory -p kask_bridge -p hkask-mcp -p hkask-mcp-curator -p hkask-mcp-corpus` | **Passed**, dev/all-targets/all-features/warnings-as-errors; machete, typos and buf checks clean. Observed wrapper elapsed 73s. |
| `cargo check --offline --locked -p zed -j 8` after T18 | **Passed**, 31.53s; retained as `target/kask-t18-zed-check.log`. |

**Scope/residue:** no new production files, crates, dependencies, settings pages, or MCP tools. Changes stay in shared corpus types, salience, bridge recall, corpus tagging/consolidation/publication/composition and their existing tests/docs. Test data is isolated below target and temporary directories; no live providers or operator-data backfill were used. The source comment corrections made after validation only clarify measurement purpose. Final explicit-file rustfmt, build-profile pins, MCP presence ratchet (0 violations/0 gaps), and diff checks passed after the documentation updates.

## Historical handoff — 2026-09-07

Read [kask-reliability-continuation-prompt.md](kask-reliability-continuation-prompt.md) first for the exact resume sequence and build evidence. HEAD remains `2475305420ae065b5d1792c0f25cea471e558ae3`; no commits or branches were created. Preserve all current changes.

**Out-of-band index mutation (observed 2026-09-07):** an external process staged the full working tree at least twice during the receiving agent's session — once before any receiving edits (outgoing agent's work + untracked task docs) and once mid-session (capturing the strengthened scenarios tests before the final fixture reorder). The receiving agent did not stage, unstage, or reset anything; worktree content was authoritative throughout and RED/restore steps used worktree-only `git show HEAD:path > path` redirection so index state was never mutated. The operator should identify what stages this tree; if it is another agent process, the single-editor rule is already being violated.

| Work | State | Next action / owner |
|---|---|---|
| T01 scenario persistence | **Verified 2026-09-07** — RED observed pre-fix, 18/18 tool-behavior tests GREEN post-fix, clippy/check clean; three persistence regressions including the new publication/truncation crash-window boundary | Receiving agent: operator review at Checkpoint A; then T02 |
| D01 policy | **Ratified YES**, recorded in both owning references; enforcement citations updated with verified evidence | Closed pending operator confirmation |
| D01 retention guard | **Verified 2026-09-07** — RED observed pre-fix (research rows destroyed), 5/5 recovery tests GREEN, 68/68 companies suite GREEN, transactional refusal/rollback confirmed | Receiving agent: operator review at Checkpoint A |
| T02 extraction destination policy | **Core slice verified 2026-09-07** — redirect-per-hop gate, connect-time validating resolver, no_proxy explicitness, address-family hardening; RED reproduced the full SSRF pre-fix | Receiving agent: operator review at Checkpoint A |
| T02b discover-path transport | **Verified 2026-09-07 (test-first RED)** — `rss_discover_feeds` now fetches through the same validated client; sentinel-redirect regression, direct-fetch control, and tool-seam pin green; clippy/check clean | Receiving agent: operator review at Checkpoint A |
| T03 forgetting coverage | **Verified 2026-09-07 (test-first RED)** — deletion now scoped to turns the watermark provably covers; newer turns + embeddings survive and stay recallable; spec §6 updated | Receiving agent: operator review at Checkpoint A |
| Storage-suite parallel flake | **Root-caused and fixed 2026-09-07 (operator directive)** — lazy on-demand pools + 120s patience + missing catalogue writer lock; suite 60/60 twice consecutively, tests ~2.6× faster | Closed |
| Dead/redundant code | **Sweep done 2026-09-07 (operator directive)** — dead budget builders and 6 pre-existing clippy failures removed; 3 designed-but-unwired capabilities flagged for operator decision | 3 flags await operator decision |
| T04–T08 | Not started | Resume after Checkpoint A operator review; retain checkpoint and task-specific policy gates |
| T09–T15 | Not started, require elaboration | Follow-up queue remains open; no accepted deferral |

**T01 test weakness (resolved):** the flagged weakness — counts checked after reopen, then re-scoring before asserting the stored outcome — is fixed as described in the T01 section. The publication/truncation crash-window boundary the review asked for is covered by `snapshot_with_uncleared_journal_recovers_exactly_once`.

**Build state (resolved):** the five 180-second timeouts were a window problem, not a code problem. With the incremental cache preserved and a 30-minute bounded window, the same command shape completed: dependency compilation finished and every test suite reached execution. `./script/clippy -p hkask-mcp-scenarios -p hkask-mcp-portfolio` (release/all-targets/all-features, `--deny warnings`, machete/typos/buf) completed clean in 35m49s. Do not return to 180-second windows for these crates.

**RED and GREEN evidence (2026-09-07):** pre-fix RED was observed against worktree-only reverts of the production files (`git show HEAD:path > path`, index untouched): all three scenarios persistence regressions failed for the intended reasons (persistence failures not surfaced to the caller), and a throwaway portfolio probe showed the unlink/recreate recovery destroying seeded research rows (`d01_red_evidence.rs`, deleted after capture). Post-fix GREEN: exact commands and outputs are recorded in the Definition-of-done evidence section below. No compilation-failure RED was claimed.

## Architecture constraints and provenance

- [Magna Carta](../kask/docs/architecture/core/magna-carta.md), lines 47–98: preserve user sovereignty and parent-held authority; distinguish intended capabilities from live enforcement. Do not add a parallel capability system.
- [Memory specification](../kask/docs/architecture/memory-system-specification.md), lines 599–675: idle-thread distillation; additive-only learning; watermark-before-lesson insertion; bounded six-hour startup discovery; single-copy, distillation-gated hard deletion. Do not turn forgetting into soft expiry or silently reverse watermark ordering.
- [Regulation D4](../kask/docs/diataxis/hkask-regulation/explanation.md), lines 22–54: human-applied advice, fresh evidence, original-threshold recovery, unverified causality, no autonomous actuator.
- [MDS](../kask/docs/architecture/core/MDS.md), sections 2 and 7: name the actual entity and reaching flow; express user-facing expectations and observable postconditions. Attach contract annotations to tests/production seams during implementation, not fictional enforcement claims in this plan.
- Existing idempotency contract: [idempotency.rs](../kask/mcp-servers/hkask-mcp-kata-kanban/src/idempotency.rs), lines 26–41, retains uncertain effects as Pending. Preserve its documented TTL rather than promising eternal exactly-once execution.
- Portfolio-only schema reset remains deliberate. D01 now explicitly forbids deleting shared research: see the ratified [portfolio recovery decision](../kask/docs/reference/mcp-servers/portfolio.md) and [companies ownership reference](../kask/docs/reference/mcp-servers/companies.md). The current guard refuses incompatible mixed databases rather than migrate or cascade-delete their research parents.
- Audit history anchors include `4dfcb76464` (hard deletion), `87652e946f` (memory deletion hardening), and `661c8e09e0` (tool behavior tests). Recover full task-specific commit intent before each implementation; the audit is not a replacement specification.

No upstream edits, generic transaction framework, or new service crate are presumed. T01 adds the existing workspace `tempfile` dependency to scenarios for same-directory atomic snapshot publication and temporary test fixtures; `Cargo.lock` records that one direct dependency edge. If an upstream change is necessary, stop to define its D-seam and regression pin. Only one agent edits the tree at a time. Read-only review may run in parallel.

## Execution protocol

Each task is a vertical slice: **contract → representative failing regression → minimal fix → control/failure variants → scoped validation**. One regression first is not a one-test ceiling. Tests must reach the real production behavior through its supported seam, with real temporary stores and controlled external-effect fixtures. No live providers, production DBs, or paid dispatches.

Before starting each task:

1. Recheck HEAD/worktree and recover callers, tests, spec history, and any applicable local rules.
2. Confirm the listed file boundary; locate an existing test seam. If scope exceeds one focused session or approximately five files, split into independently useful behavioral slices before editing. T02, T04, T05, and T11 particularly require this sizing check.
3. State the user expectation, motivating principle, applicable constraints, preconditions, postconditions, and measurable falsifier. Unknown user-visible behavior goes to an operator gate, not an implicit default.

T01 and D01 test names in the handoff now exist but have not executed. Other test names/fixtures below remain planned. Sizes S/M are provisional, not time promises.

## First tranche: concrete regressions

### Phase A — Protect boundaries and recoverable data

### T01 — Preserve scenario recovery on snapshot failure

**Scope:** M; lifecycle; `hkask-mcp-scenarios`. **Depends on:** none.
**Likely files:** `kask/mcp-servers/hkask-mcp-scenarios/src/superforecast/store.rs`, `src/hkask_mcp_scenarios.rs`, `tests/tool_behavior.rs` (latter paths relative to that crate).
**Anchor:** `ForecastStore::compact`, `scenario_score` → journal → snapshot → reopen.
**Status: verified 2026-09-07.** `save_entry`, `insert`, and `persist` propagate I/O errors; memory changes follow successful journal append; a synced same-directory `NamedTempFile` snapshot is published before journal cleanup, with Unix directory sync. The tool surfaces partial-progress/persistence errors. The review-flagged test weakness is fixed: `snapshot_failure_preserves_journal_for_recovery` now asserts recovered identity/probability/outcome through `scenario_status`'s `recent_forecasts` read seam BEFORE any re-scoring, and compares the retained journal's full two entries (pending insert + resolved update) instead of non-emptiness; it also asserts the failed publication leaves no temporary-file residue. New boundary regression `snapshot_with_uncleared_journal_recovers_exactly_once` covers the crash window between snapshot publication and journal truncation: a published snapshot with an uncleared overlapping journal recovers exactly once, with the journal's last write winning over a stale snapshot version (fixture uses production-written bytes for both files). The prior-snapshot-survives-failed-replacement property is design-reviewed but not test-pinnable — see the scenarios reference persistence section.

Acceptance:
- A failed snapshot write/publication leaves the prior recovery journal intact; reopening recovers every acknowledged durable record exactly once.
- Successful compaction publishes a complete replacement before journal cleanup; failure at publication and failure-then-retry preserve recoverability.
- The public scoring response surfaces persistence failure/degradation, rather than ordinary durable success; successful scoring behavior remains unchanged.

**Verification:** file-backed public-tool round trip with injected write/publication failures and a successful control. Use deterministic filesystem/driver faults, not permission assumptions that disappear when run as root. Assert exact recovered records and journal bytes. Explicitly distinguish tested I/O failure recovery from untested power-loss guarantees.
**Refused shortcut:** log snapshot failure then clear the journal, or add a test over an in-memory-only store.
**Skill match query:** persistence fault isolation and regression-first recovery testing.

### T02 — Enforce extraction destination policy across redirects

**Scope:** M; trust; `hkask-mcp-research`. **Depends on:** none.
**Likely files:** `kask/mcp-servers/hkask-mcp-research/src/research/providers.rs`, `src/research/providers/raw_fetch.rs`, `tests/tool_behavior.rs`; shared `hkask-mcp-server/src/security.rs` only if required by the real connection-validation seam.
**Anchor:** `web_extract` → `ProviderPool::extract_with_fallback` → RawFetch transport.
**Status: core slice verified 2026-09-07; follow-up slice T02b open.** Implemented at the real connection-validation seam, as the likely-files list anticipated:

- `RawFetchProvider` now owns a validated client (`raw_fetch_http_client`): a custom reqwest redirect policy re-runs the strict literal checks on **every redirect hop**, bounds the chain (10 hops) and refuses cycles; a validating DNS resolver (`reqwest::dns::Resolve` over `hkask_mcp_server::server::validate_resolved_addresses`) gates every connect-time resolution, closing the DNS-rebinding TOCTOU at this transport; `.no_proxy()` makes the connected destination always the validated one (surfaced with a build-time warn when proxy env vars are present). Redirect-fetched content is labeled with the final URL. RawFetch errors now carry the full reqwest cause chain (`reqwest_error_detail`) so a rejected hop names the reason.
- `hkask-mcp-server/src/security.rs` (shared seam, required): address policy extended with unspecified destinations (`0.0.0.0/8`, `::` — connecting to 0.0.0.0 routes to loopback on Linux) under the private gate, and NAT64 (`64:ff9b::/96`) / IPv4-compatible unmasking so neither family can re-spell a forbidden IPv4. The resolved-address loop was extracted (`validate_resolved_addresses`) and two public entries added (`validate_tool_url_literal` for the sync redirect-policy hop check, `validate_resolved_addresses` for the resolver), re-exported via `server.rs`. **The permissive (user-curated RSS) policy is byte-identical** for all new checks.

Acceptance:
- A public-to-private/loopback redirect never reaches the forbidden sentinel, including a multi-hop chain.
- Permitted redirects and direct public extraction still work; loops/redirect limits produce bounded, surfaced failure.
- The validated destination is the connected destination; resolver changes, address-family variants, and configured proxy behavior cannot silently bypass the gate. Unsupported transport configurations must be explicit, not falsely certified secure.

Coverage against acceptance (see research.md "Extraction destination policy" for the full boundary statements): forbidden single hops (loopback literal, 169.254.169.254, `localhost` name) are E2E-proven to make zero sentinel requests; chain bound, cycles, scheme/credential redirects, and permitted public-literal hops are pinned by the redirect-decision tests (a permitted multi-hop chain through locally-servable hops is not constructible under the strict gate — every local address is forbidden by policy); per-hop enforcement applies uniformly at every chain position via the same closure. Third-party provider APIs fetch target URLs server-side from their own network position — that fetch is not this transport and is documented as out of this gate.

**Verification:** real extraction transport with deterministic DNS/connection fixtures preserving production validation. Do not weaken the validator to make a loopback fixture pass. Check the exact pinned reqwest fork. No live attacker endpoint is needed. — **Executed 2026-09-07**: fork APIs confirmed (`redirect::Policy::custom`, `ClientBuilder::dns_resolver`/`no_proxy` — the builder method is `redirect_policy` on the zed fork); fixtures are loopback listeners with request counters, zero network. RED observed against worktree-only reverts (index untouched): the transport probe reproduced the full SSRF — pre-fix RawFetch followed the forbidden redirect and returned the sentinel's content (`Ok("sentinel-body")`) — and the three new tool-seam gate tests failed because `0.0.0.0`/NAT64 destinations passed validation. GREEN: research lib 34/34, tool_behavior 30/30, hkask-mcp-server lib 22/22; `cargo check` and `./script/clippy -p hkask-mcp-server -p hkask-mcp-research` clean.
**Refused shortcut:** initial-URL-only validation, disabling all redirects without reviewing compatibility, or mocking out the HTTP path under test. All refused as designed; redirects remain followed (validated per hop), the real transport is exercised over real sockets.
**Skill match query:** SSRF boundary testing through redirect and DNS transport behavior.

**T02b — follow-up slice (same class, separate wiring): verified 2026-09-07.** `rss_discover_feeds` validated its initial URL strictly but fetched through the shared `rss_client` (default redirect following). RED was observed test-first before the fix (a throwaway probe in `feed.rs` showed `discover_feeds` with the handler's client class following a redirect into a loopback sentinel and returning its content). Fix: the validated client construction is shared (`validated_fetch_client`, now `pub(crate)` from `raw_fetch.rs`, re-exported through `providers.rs`/`research.rs`), a `discover_client` field is wired through `ResearchServer::new` (all five call sites updated — production `run()` plus four test fixtures), the discover handler fetches through it, and its reqwest cause chain is rendered so a rejected hop names its reason. Permanent regressions: `discover_feeds_rejects_redirect_to_loopback` (sentinel counter zero, reason named) and `discover_feeds_direct_fetch_still_works` (validated client does not change direct fetches) in `feed.rs`, plus the tool-seam pin `rss_discover_feeds_rejects_unspecified_destination`. The wiring itself (handler passes `discover_client`, not the permissive `rss_client`) is a distinct typed field pinned by construction and review — a full tool-seam redirect E2E is not locally constructible (the strict outer gate rejects every locally-bindable initial URL). `rss_subscribe`/`rss_fetch`/`rss_synthesize` fetch with the permissive policy by ratified design; their redirects to local destinations are policy-permitted, not a defect.

### T03 — Restrict forgetting to distilled content

**Scope:** M; lifecycle; `hkask-mcp-curator`, storage seam if necessary. **Depends on:** none.
**Likely files:** `kask/mcp-servers/hkask-mcp-curator/src/forgetting.rs`, `src/distillation.rs`; existing memory/storage deletion implementation only if atomic eligibility/deletion needs it.
**Anchor:** distillation timer → forgetting → stored chunks/embeddings → semantic recall.
**Status: verified 2026-09-07.** The defect: the pass deleted **every** shared turn of an eligible thread by entity prefix — including turns added after the last distillation pass, whose lessons were never extracted, and any turn racing the pass. Fix (coverage-scoped deletion): a turn is deletable only when its `observed_at` ≤ the newest watermark's `through` position — the same boundary distillation uses to select pending turns (`parse_watermark_through`, shared with `distillation.rs`). Fully-covered threads keep the whole-entity fast path (identical prior behavior); mixed threads delete covered h_mems by id and their embeddings by passage match, excluding passages an uncovered chunk shares (ambiguity resolved conservatively: keep the embedding — a bounded duplicate leak beats erasing an uncovered chunk's recall). A watermark whose `through` cannot be parsed skips the thread (no proof, no deletion). New storage seams: `EmbeddingStore::delete_by_entity_ref_and_passages` + `MemoryStore::delete_embeddings_by_entity_passages`, transactional per the existing delete pattern.

Acceptance:
- With an aged watermark and newer turns, newer turns and their embeddings remain recallable after forgetting. — Pinned E2E: `forgetting_preserves_turns_newer_than_the_watermark` asserts the surviving chunk is semantically recallable via the real KNN seam and the forgotten one is not.
- Eligible covered content is still hard-deleted; never-distilled threads and watermark records remain protected; repetition is idempotent. — Existing pins held with coverage-faithful fixtures (turn timestamps now precede the watermark's `through`; the prior fixtures modeled turns as covered while their timestamps postdated the watermark — the defect baked into fixtures).
- A controlled insertion between eligibility evaluation and deletion cannot erase the newly inserted content. — Structural: the pass deletes only rows it READ as covered (per-id and passage-scoped deletes, never a blind prefix delete), so a racing insertion (`observed_at` > `through` by construction) is uncovered and survives.

**Verification:** executed 2026-09-07, test-first: the three new regressions were written and observed failing against the pre-fix pass (2-turns-deleted instead of 1; 3 instead of 2; no-proof thread forgotten) before the fix; post-fix GREEN: curator lib 16/16 (forgetting 7/7), hkask-memory 30/30, hkask-storage 60/60 (see the addenda below for that suite's own fixes); `./script/clippy -p hkask-storage -p hkask-memory -p hkask-mcp-curator` clean; spec §6 updated in the same change with the coverage semantics and the passage-attribution boundary.
**Refused shortcut:** replacing hard deletion with expiry or independently checking then unconditionally deleting the entity. Deletion remains hard, and the entity-level delete survives only for the fully-covered case where everything it holds is proven distilled.
**Skill match query:** watermark coverage and concurrent memory-lifecycle regression testing.

**Checkpoint A:** T01–T03 regression evidence, affected crate tests/checks/lints, residue review, and operator review. These three tasks are technically independent; listed order is scheduling, not an artificial dependency.

### Phase B — Make completion and retry states reliable

### T04 — Revisit pending distillation work

**Scope:** M; curation/lifecycle; `hkask-mcp-curator`. **Depends on:** T03 for the combined memory safety regression checkpoint; cursor repair itself is independently implementable.
**Likely files:** `kask/mcp-servers/hkask-mcp-curator/src/distillation.rs`, `src/thread_turns.rs`, existing inline tests.
**Anchor:** `last_pass` → shared-turn discovery → idle check → inference → watermark.

**Status: implemented and verified 2026-09-08 (Checkpoint A ratified; Phase B entry approved).**

**Root cause:** the timer loop advanced `last_pass = Some(now)` unconditionally after every pass. A thread skipped as active (idle check), or failing before its watermark advanced (inference/parse/watermark-store error), had turns observed before the new cursor value — the next pass's `valid_from >= since` scan (which filters on `observed_at`, the value `HMemStore::insert` writes into `valid_from`) could never see it again. The thread was permanently skipped until a new turn arrived or the server restarted (6h lookback). A pass that could not read the store also advanced the cursor, so turns observed during an outage fell behind it on heal.

**Fix:** cross-pass state extracted into `DistillationCursor { last_pass, pending }`. `distill_store` gains a `revisit` parameter (pending threads' complete turn sets reloaded via the existing `thread_turns` seam — a superset of the window scan) and reports per-thread dispositions: `scan_failed` (store unavailable or scan query error — the cursor must not advance) and `threads_pending` (still-unresolved threads). `DistillationCursor::merge` advances the cursor only on a successful scan, carries pending threads with preserved first-seen times, and enforces `MAX_PENDING_THREADS = 128` — overflow evicts the longest-pending thread with a `warn!` naming it and its re-discovery paths (a new turn, or the restart lookback: exactly the pre-T04 status quo for that thread). `run_pass` is the production orchestration the timer calls; `spawn_distillation_timer` now takes `DistillationConfig` as a parameter (env read moved to the call site) so the timer itself is testable. The obsolete `cadence.clamp(60, 3600)` poll cap became `cadence.max(60)` — the same one-hour-cap defect class the operator's T17 consolidation ruling removed; the cursor (not the poll interval) bounds what a pass sees, so this is config honesty with no data effect. Pending state is in-memory only: restart clears it and the bounded 6h lookback re-discovers recent work (the older-than-lookback miss boundary is unchanged). Watermark-before-lesson ordering, additive-only mutation, and the bounded startup lookback are untouched; the full existing distillation suite passes unchanged.

Acceptance:
- A turn skipped as active becomes eligible and is distilled on a later production-shaped pass without requiring a new turn or restart. — `later_pass_distills_thread_that_went_idle_without_a_new_turn` (RED pre-fix: 0 distilled vs 1).
- A transient inference failure before watermark advancement is retried; successful work is not replayed unnecessarily. — `transient_inference_failure_is_retried_on_the_next_pass` (RED pre-fix: 0 retried vs 1); `cursor_progresses_and_successful_work_is_not_replayed` control (distilled thread not re-examined; new turn picked up; single lesson/watermark per thread).
- Ratified watermark-before-lesson ordering and bounded startup discovery remain intact. Pending-work bounds/overflow must be explicit and non-silent. — `first_pass_scan_is_bounded_to_the_startup_lookback` control; `pending_threads_are_bounded_with_explicit_eviction` (129 pending → bounded 128, newest survives, survivors distilled once idle; RED pre-fix: no pending set existed). `scan_failure_does_not_advance_the_cursor` pins the outage path (RED against HEAD's unconditional loop advance). Timer-level: `distillation_timer_fires_after_the_first_interval_and_distills` (T17's lesson — the spawned timer actually fires; first tick skipped) and `distillation_timer_honors_cadences_longer_than_one_hour` (RED pre-fix: the clamp fired a 7200s cadence at 3600s).

**Verification (executed 2026-09-08):** RED observed by temporarily restoring pre-fix cursor semantics (unconditional advance, pending dropped, clamp restored, HEAD's loop-level advance on store-unavailable) — all five behavioral regressions failed for their intended reasons while the controls and the six existing distillation tests passed. GREEN after restore: `cargo test --offline --locked -p hkask-mcp-curator --lib -j 8` → **26 passed, 0 failed** (18 existing incl. T03 forgetting pins and T16 store tests + 8 new). `cargo check --offline --locked -p hkask-mcp-curator -j 8` clean; `HKASK_BUILD_JOBS=8 CARGO_NET_OFFLINE=true ./script/clippy --locked -p hkask-mcp-curator` clean (dev/all-targets/all-features/`--deny warnings`, machete/typos/buf); rustfmt clean on both touched files. Tests drive the production orchestration (`run_pass` with the real cursor, real in-memory SQLite stores, scripted inference, controlled pass times) — not an epoch-wide helper scan. Tokio `test-util` added to the crate's dev-dependencies for the paused-time timer tests (same addition T17 made for kask_bridge).

**Observation (not a defect, recorded for the T09 identity class):** the watermark and turn reads are entity-PREFIX queries (`h_mems_by_entity_prefix("curator:distilled:{id}")` / `thread_turns`), so a thread id that is a proper prefix of another (e.g. `t1` vs `t10`) would cross-contaminate watermarks. Production thread ids are `acp::SessionId` UUID strings (`Uuid::new_v4()`, fixed length — no UUID prefixes another), so the collision is unreachable in production; the test fixture uses zero-padded ids. A future non-UUID thread-id source would need exact-match reads.

**Refused shortcut:** moving the cursor without tracking unresolved work, or silently rescanning all historical memory. (Also refused: persisting pending threads as h_mems — scheduler bookkeeping does not belong in the additive-only memory store; and an unbounded pending set — silent growth during a long inference outage.)

### T05 — Reserve session authorization before external dispatch

**Scope:** M, re-slice if necessary; trust; `hkask-mcp-swarm`. **Depends on:** none. **Policy gate:** RESOLVED — the operator ratified the settlement policy at T05's start (2026-09-08): "Q1 A; Q2 as recommended; Q3 fix all three; Q4 as recommended."

**Likely files:** `kask/mcp-servers/hkask-mcp-swarm/src/spend_gate.rs`, `src/consent.rs`, existing server/consent tests.
**Anchor:** `authorize_delegate` → external POST → `complete_delegate` → durable consent state.

**Status: implemented and verified 2026-09-08.**

**Root cause (two defects, both confirmed in code):** (1) Session spends validated with cost=0 and a NON-ATOMIC `session_balance` read at authorize time, deducting only in `settle_success` AFTER the external POST — two overlapping 10-credit authorizations against a 10-credit session both passed the check, both POSTed, and the second deduction failed with only a `warn` ("session balance may be stale"): external double-dispatch against one session's capacity, including across separate server processes sharing the SQLite consent DB. (2) Every transport error auto-refunded single-use tokens — including timeouts where ABW may have ACCEPTED the dispatch: external spend plus a refunded, re-usable token. All transport errors collapsed to `Unavailable(String)`, discarding reqwest's `is_connect()` classification (never-sent vs sent-no-answer).

**Fix:** `Settlement` now carries an explicit reservation taken at `authorize_*` time — single-use: the consumed token IS the reservation (unchanged); session: the re-verified cost is ATOMICALLY DEDUCTED via the store's existing conditional-UPDATE (`consume_session` with the real cost), replacing the non-atomic balance read and the post-dispatch deduction; `settle_success` is deleted (the reservation IS the spend). `ConsentStore::release_session` (new, both backends, warn-on-failure) credits a reservation back. Dispatch failures settle by classification: `SwarmError::is_proven_rejection()` (new) — connect-phase/construction failures (`DispatchNotSent`, new variant), every ABW error response (existing variants), and the 200-envelope upstream error (ABW's own operation-failed statement) — releases; everything else (`DispatchAmbiguous`, new variant: timeout/reset/lost response; `ApiVersionMismatch`: accepted-but-unparseable) HOLDS the reservation and surfaces the uncertainty ("inspect the ABW workspace before retrying; a retry cannot reuse this authorization"). `abw_client::send` classifies transport errors at the reqwest seam (`is_connect() || is_builder()`) and body-read failures by response status. The xaman guard (`CuratorSession`) settles by the same classification at its three failure points; its `Drop` (send never called — nothing dispatched) releases. All three paths (delegate, hire+`/hire`→`/add` fallback, xaman) route through the shared `settle_dispatch_failure`. `session_balance` lost its last production callers and was removed (tests read balances via `consume_session(_, _, 0)`, a zero-cost validating read).

Acceptance:
- Two overlapping 10-credit authorizations against one 10-credit session admit at most one POST, including separate server instances sharing the consent DB. — `overlapping_session_authorizations_admit_at_most_one_dispatch` + `overlapping_hire_authorizations_admit_at_most_one_dispatch` (two `ConsentStore`s on one SQLite file; RED pre-fix: "both authorizations succeeded — the session was oversubscribed").
- Success settles once; a proven pre-dispatch rejection releases only its own reservation; per-dispatch ceilings and single-use consent behavior remain intact. — `successful_dispatch_reserves_exactly_once` (no double deduction), `connection_refused_releases_session_reservation`, `http_rejection_releases_session_reservation` (controls).
- External acceptance followed by transport/local-persistence failure retains durable reservation evidence across reopen and reports uncertainty; a retry cannot reuse the same reserved capacity. — `ambiguous_dispatch_holds_session_reservation_across_reopen` (credits stay deducted, hold survives a fresh store instance, retry refused), `ambiguous_dispatch_keeps_single_use_token_consumed` (Q4: token stays consumed across reopen), `xaman_create_ambiguous_holds_curate_token`, `xaman_send_ambiguous_holds_curate_token`, `xaman_create_without_session_id_holds_curate_token` (accepted-but-unreadable), with `xaman_rejection_releases_curate_token` and `xaman_success_consumes_curate_token` as controls. No external exactly-once promise is made — the uncertainty message tells the operator to inspect the workspace first.

**Verification (executed 2026-09-08):** RED observed against pre-fix semantics in two passes (both mutations restoring HEAD's behavior, then reverted): pass A (unconditional refund on every dispatch error) — the five ambiguity/accepted-unreadable tests failed for their intended reasons ("the uncertainty must be surfaced", "the curate token must stay consumed") while race/control/success tests passed; pass B (validate-only, no session reservation) — both race tests panicked "oversubscribed" and the reservation-dependent tests failed. GREEN after restore: `cargo test --offline --locked -p hkask-mcp-swarm --lib -j 4` → **133 passed, 0 failed** (121 existing + 12 new). `HKASK_BUILD_JOBS=4 CARGO_NET_OFFLINE=true ./script/clippy --locked -p hkask-mcp-swarm` clean; rustfmt clean on all five touched files. The HTTP fixture is a bounded local TCP server (`abw_client::test_http`) serving a fixed behavior sequence — never a real provider. `tempfile` added as a dev-dependency (T04 precedent).

**Incidental fixture fix found by the RED protocol:** the fixture's Drop-time shutdown self-connect could consume a queued behavior when a test ended before serving all of them (a mid-test assertion failure would HANG teardown instead of failing cleanly — observed live under mutation B, diagnosed via /proc wait-channels: the accept thread never woke). Fixed with a shutdown flag checked after accept; the mutation-B rerun then failed cleanly for the intended reason. This hardening applies to every future test using the fixture.

### T06 — Preserve replay protection after agent creation

**Scope:** M; composition/lifecycle; `hkask-mcp-kata-kanban`. **Depends on:** none.
**Likely files:** `kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs`, `src/idempotency.rs`, `tests/idempotent_creates.rs`.
**Anchor:** `with_idempotency` → successful spawn → fallible task comment → same-key retry.

**Status: implemented and verified 2026-09-08.**

**Root cause:** both spawn paths returned `Err` from the post-spawn `task_comment` (`.map_err(map_kanban_error)?`) — AFTER the external effect (the worktree agent thread exists / the delegation executed). `with_idempotency`'s Err arm releases the claim on the assumption "nothing landed" — false here: the same-key retry re-ran the spawn and created a SECOND agent (a second worktree thread / a second delegation spend), the exact duplication the protection exists to prevent.

**Fix:** post-spawn `task_comment` failures are non-fatal in BOTH paths (warn + the new optional `TaskSpawnResponse.result_note_error` field carries the reason — wire-compatible, absent when the note was recorded) — uniform with the already warn-only `task_move`/`task_record_delegation`. Every remaining `Err` from the spawn closure is provably pre-effect (id/level validation, config write, task read, runtime init, delegation failure), so the wrapper's release-on-Err is correct again; the `with_idempotency` doc contract now states this explicitly ("work must return Err only when NO effect landed"), and `idempotency.rs`'s module doc records the partial-response rule. The partial response is RECORDED, so a same-key retry replays it (one agent); if the record itself fails, the claim stays Pending ("outcome unknown", refused — no re-run).

Acceptance:
- Post-spawn bookkeeping failure followed by same-key retry creates exactly one agent; result is replayable or explicitly pending/partial. — `post_spawn_comment_failure_keeps_replay_protection`: a one-shot `task_comment` fault (the pre-spawn config comment writes directly, so the fault fires exclusively post-spawn) fires after the counting worktree port has spawned; the call returns Ok with `result_note_error` set; the same-key retry REPLAYS it with the partial marker intact; the port count stays 1. RED pre-fix: the call failed after the effect landed.
- Concurrent same-key requests and retries after reopening durable state preserve the claim within the existing TTL, including failure after external acceptance before response recording. — `concurrent_same_key_spawns_admit_one_agent` (tokio::join!: exactly one spawn; the loser replays or is refused with the pending verdict); `pending_claim_survives_reopen_and_refuses_the_spawn` (a directly-reserved, never-recorded claim on the shared SQLite driver survives a second server instance — the restart — and REFUSES the retry; zero spawns). Cross-process replay durability itself is pinned by the existing `replay_is_absorbed_across_processes`.
- Proven clean pre-effect validation failures release the claim and can be retried successfully; ephemeral-goal semantics remain unchanged. — `pre_effect_failure_releases_the_claim_for_a_clean_retry` (control: unparseable task id errs pre-effect, same key then runs fresh on the corrected input); the goal tests (`goal_replay_protection_does_not_survive_a_restart`, `goal_replay_is_absorbed_within_the_process`) pass unchanged — the ephemeral goal store is untouched.

**Verification (executed 2026-09-08):** RED observed by temporarily restoring the fatal post-spawn comment — `post_spawn_comment_failure_keeps_replay_protection` failed at "the spawn itself succeeded" (the intended reason); the concurrency/pending/pre-effect tests passed under the mutation (they pin unaffected semantics). GREEN after restore: `cargo test --offline --locked -p hkask-mcp-kata-kanban -j 4` → **46 lib + 21 idempotent_creates + 1 rjoule pin, 0 failed**. `HKASK_BUILD_JOBS=4 CARGO_NET_OFFLINE=true ./script/clippy --locked -p hkask-mcp-kata-kanban` clean; rustfmt clean on all five touched files. The one-shot fault is a `#[doc(hidden)]` test seam (`KanbanService::fail_next_comments` — an Arc-shared atomic counter checked only in `task_comment`; zero production readers), following the `CuratorDb::from_stores` doc-hidden precedent. The local-runtime path's post-spawn comment received the identical edit; it is not E2E-testable under the harness (the local delegation needs a wired inference port — the existing `spawn_is_not_blocked_by_an_unfunded_ledger` test pins its pre-effect refusal), and the worktree path carries the behavioral proof through the same `with_idempotency` seam.

**Refused shortcut:** classifying all errors as clean (the pre-fix behavior — the exact defect), or testing only an unavailable spawn port (the counting port makes a duplicate spawn observable as count > 1).

**Checkpoint B:** cumulative memory, authorization, and spawn regression checks; scoped build/lint evidence; explicit resolution of T05's policy gate; operator review. Serial editing is a resource constraint, not a T04→T05→T06 technical dependency. **Status:** evidence complete 2026-09-08; the review opened but was superseded by the Phase E teardown ruling; re-presented 2026-09-09 with the teardown accounting (plan.md §Phase E) — **RATIFIED by the operator 2026-09-09** (all four decision items: the T04 pending-set bound of 128 with evict-oldest + warn; the C4 swarm-detail deletion as completing the compose replacement; the fire-from-panel capability placement — `swarm_fire` remains an MCP/Steer-path tool; the T06 `fail_next_comments` doc-hidden test seam). Phase C (T07 → T08) unblocked.

### Phase C — Make regulation acknowledgments truthful

### T07 — Report actual directive outcomes — VERIFIED 2026-09-09

**Scope:** M; curation; `hkask-regulation`. **Depends on:** none.
**Likely files:** `kask/crates/hkask-regulation/src/cybernetics_loop/directive.rs`, existing regulation tests; acknowledgment consumers only if the recovered schema requires it.
**Anchor:** directive inbox → application handler → acknowledgment/event sink.

**Defect found (pre-fix):** every non-dampened directive persisted a `CurationDirectiveAcknowledged` record with `outcome: "applied"` regardless of what happened. Four of eight variants lied: `UpdateCapabilities` and `SeekMoreEvidence` logged only (no capability-mutation or evidence-seeking handler exists) but claimed "applied"; `EscalateDomain` was silently swallowed by the `_ => {}` catch-all AND acknowledged "applied"; `EvolveMcpToolSchema` persisted its honest `outcome: "recorded"` payload record and THEN a second, false generic "applied" record on top. No consumer parses the outcome field yet (verified by grep — schema change is safe), so the lie was pure future corruption of the audit trail.

**Fix (delete the false paths, not add new ones):** `apply_directive` returns a `DirectiveOutcome` (`Applied`/`Recorded`/`LogOnly`/`Unsupported`) and `persist_directive_acknowledgment` persists that truth. The always-"applied" literal is deleted; the duplicate evolve acknowledgment is deleted (the `Recorded` arm skips the generic ack — its payload record IS the acknowledgment); the `_ => {}` catch-all is deleted in favor of explicit arms for every variant, so adding a `CuratorDirective` variant without choosing an outcome is a compile error, not a silently-ignored directive acknowledged as applied. `EscalateDomain` now honestly reports `unsupported` with a warn (T08 wires the real delivery path). Dampened directives still produce no acknowledgment (dampening ≠ application — pinned). No capability authority or autonomous actuator was added to make any acknowledgment true.

**Verification (executed 2026-09-09):** RED observed pre-fix — `directive_acknowledgments_report_actual_outcomes` failed at "exactly one acknowledgment per directive" (6 records for 5 directives: the evolve false duplicate). GREEN post-fix: the full variant table through `process_inbox` with a capturing `RegulationSink` — real effects asserted on live state (override installs ceiling 5 → `(5,5)`; replenish credits 3 after consuming 8 of 10 → `(10,5)` — credit saturates at the ceiling, so the test consumes first; clear restores `(10,10)`), log-only variants report `log_only`, `escalate_domain` reports `unsupported`, the evolve request reports `recorded` exactly once with its payload. Controls: `dampened_directive_is_not_acknowledged` (repeat within the 60s window → no ack) and `persist_failure_does_not_wedge_the_inbox` (failing sink → warn, inbox drains, no panic). Suite: `cargo test -p hkask-regulation` → **66 passed, 0 failed** (63 existing + 3 new). `./script/clippy -p hkask-regulation` exit 0, zero warnings; rustfmt clean. Test-design note: the metacognitive override cooldown (120s — any metacognitive directive passing dedup suppresses ALL overrides) means the cap variants run on a second loop instance; that cooldown is existing designed dampener behavior, not the behavior under test.

**Net-LOC:** +383 in `directive.rs` (+86 production: the `DirectiveOutcome` enum and the explicit per-variant arms with their why-comments; +297 test: the required variant-table evidence). The false-acknowledgment paths are deleted; the addition is the truth-valuing structure the task spec required ("failures and unsupported/log-only variants are explicitly distinguished"). No external consumers of the outcome field (grep-verified), so no compatibility review was triggered.

Acceptance:
- Applied acknowledgment requires evidence of the actual supported effect; failures and unsupported/log-only variants are explicitly distinguished. ✓
- Dampened input is not represented as applied; working cap/threshold operations preserve their behavior. ✓ (cap operations' live-state assertions pass unchanged)
- No capability authority or autonomous actuator is added to satisfy a formerly misleading acknowledgment; incompatible acknowledgment schema changes require review. ✓ (no consumer exists; field values changed from always-"applied" to truthful — recorded here as the review)

**Verification:** table of existing directive variants through inbox processing with real state assertions and a recording event sink; handler failures must not become success.
**Refused shortcut:** rename a log message while continuing to persist false completion.
**Skill match query:** typed command outcomes and production dispatch contract tests.

### T08 — Deliver explicit domain escalations — VERIFIED 2026-09-09

**Scope:** M; curation; regulation/bridge seam. **Depends on:** T07 (done).
**Anchor:** `EscalateDomain` → inbox → existing `AlertEscalationSink` / `BridgeAlertEscalationSink` durable queue and existing alert channel.

**Defect found (pre-fix):** `EscalateDomain` was silently swallowed by the `_ => {}` catch-all AND acknowledged "applied" (the T07 finding). The delivery infrastructure existed — `alert_escalation_sink` on the loop, `BridgeAlertEscalationSink` forwarding to the reviewable `EscalationQueue` (`curator_escalations` MCP tool) — but nothing wired the directive to it, and the sink contract (`persist_alert` → `()`) could not prove persistence.

**Fix:**
- **Narrowly adapted the sink contract** (the spec's instruction): `AlertEscalationSink` gains `try_persist_alert → Result<AlertQueueOutcome, String>` with a default that falls back to best-effort `persist_alert` and returns `Attempted` — existing sinks keep their contract. `AlertQueueOutcome::Confirmed(Option<id>)` / `Attempted` keep confirmed, attempted, and failed distinct. `BridgeAlertEscalationSink` refactors its supersede+insert body into a reporting core; both the legacy and reporting methods delegate to it.
- **Wired `EscalateDomain`** (`apply_escalate_domain` in directive.rs): delivers to the escalation queue via `try_persist_alert`, retaining domain/severity/evidence. The queue entry is marked `explicit: true` and carries NO deficit/threshold fields — an explicit concern is a request for review, not a fabricated measured threshold breach. The message's condition key ("Explicit escalation ({domain}, {severity})") is stable per concern: a re-raised concern supersedes the pending row (latest evidence, retry_count+1) instead of duplicating; different concerns get their own rows; rapid repeats are handled by the directive dampener. Severity→confidence: info 0.25, warning 0.5, critical 1.0.
- **The acknowledgment reports the delivery truth**: `queued` (+ the queue-assigned escalation id) / `attempted` (best-effort sink or failed write — surfaced via warn) / `missing_sink` (no sink wired — surfaced via warn), with the domain/severity/evidence payload merged into the ack record (the archive copy). The dead `Unsupported` outcome variant was deleted (T08 wired its only user).
- **Routing decision returned to the operator** (per the spec: "If the existing sinks cannot faithfully carry an explicit concern, return the routing decision to the operator rather than invent policy"): the live `CurationInput` channel only carries `RuntimeAlert` (measured deficit/threshold); dressing an explicit concern as one would fabricate a sensor reading. The queue is the human-review path of record; the ack record is the archive copy. If you want live-channel notification for explicit escalations too, that needs a new `CurationInput` variant — your call, not invented here.

**Verification (executed 2026-09-09):** RED observed pre-fix — the five new directive-level escalation tests failed against the "unsupported" state. GREEN post-fix: directive level (hkask-regulation, 71 passed) — confirmed-in-queue (payload identity, no fabricated deficit/threshold, confidence mapping, ack carries id+identity), supersede-reports-queued-without-id, missing-sink surfaced, failed-write → attempted, best-effort-sink → attempted; bridge level (kask_bridge, 185 passed) — `try_persist_alert` against a REAL in-memory `EscalationQueue` (insert → Confirmed with a readable id; supersede → Confirmed(None), exactly one pending row), legacy `persist_alert` still writes through the core, and the **end-to-end**: `EscalateDomain` → inbox → real `BridgeAlertEscalationSink` → real queue, asserting the queue row's identity fields AND the ack's `escalation_id` matches the real row's id. Existing dampening/general-alert tests unchanged and green (the algedonic path is untouched — `persist_alert_to_queue` still calls the legacy method). `./script/clippy -p hkask-regulation -p kask_bridge` exit 0, zero warnings; rustfmt clean. One unreproduced flake: a single kask_bridge test failed once in an early run and passed in 9 consecutive reruns plus the final captured run (185/185, exit 0) — not attributable to this change (the kask_bridge delta is confined to alert_escalation.rs, whose tests were stable in every run).

**Net-LOC (T07+T08 combined, commit `7ff4ffca3d` + worktree):** 4 files, +989/−49 — directive.rs +743 (≈+180 production: the outcome enum, the escalation delivery, the payload merge; ≈+560 tests: the T07 variant table + the five T08 delivery tests), alert_escalation.rs +342 (≈+70 production: the reporting core + trait override; the rest tests incl. the end-to-end), algedonic.rs +43 (the `AlertQueueOutcome` enum + `try_persist_alert` default), hkask_regulation.rs +2 (re-export). The false-acknowledgment paths are deleted; the additions are the delivery wiring and its evidence.

Acceptance:
- An undampened explicit escalation retains domain, severity, and evidence in the human-review path; queue persistence and alert delivery tested independently. ✓ (queue row + ack asserted separately; the end-to-end composes them)
- Missing/broken sinks are surfaced; queued, attempted, and confirmed durable delivery are not conflated. ✓ (`AlertQueueOutcome` makes the three states structurally distinct; the best-effort default returns `Attempted`, never `Confirmed`)
- Existing dampening/general alerts remain functional, without treating a requested escalation as a fabricated measured threshold breach. ✓ (all 63 pre-existing regulation tests green; the explicit queue entry carries no deficit/threshold fields)

**Checkpoint C:** cumulative directive and memory/budget regressions, build/lint evidence, and operator review before scheduling the follow-up queue. **RATIFIED by the operator 2026-09-09 "as is"** — Phase C evidence accepted; the queue-only routing for explicit escalations stands (no live-channel `CurationInput` variant); the follow-up queue (T09–T15) opens for elaboration + scheduling.

## Phase E — Complexity teardown (operator ruling 2026-09-08)

**The ruling:** the program had been net-additive (T04–T06 added ~1,350 lines, deleted ~330) while the operator's standing 2026-09-04 budget-deprecation ruling went unenforced in code the program touched — "you have been building on unreliable code you never cleaned." Cleanup is now the program: every slice must leave the tree simpler than it found it; net-LOC accounting, simplification review, and residue removal are part of every change's definition of done. Phase C (T07/T08) is halted until the teardown tranche lands.

**Role correction recorded (operator, 2026-09-08):** the PM defines required functionality and required variables; the technical program manager owns everything below that and answers for implementation choices. Technical decisions are made and documented, not deferred upward.

### C1 — Remove the local budget system — VERIFIED 2026-09-08

**Ratified:** "delete it all - there was never supposed to be local budget for swarm."

**What was removed (operator ruling 2026-09-04 + the rJoule removal's unfinished tail):**
- The `hkask-ledger` crate entirely (workspace member, dep, files).
- `LocalSwarmRuntime`'s ledger/cost machinery: the ledger open/init, `balance()`, `history()`, `fund()`, `record_spend()`, `debit_and_build` (→ `build_result` — measurement only), the per-dispatch ceiling checks, and `credits_authorized`/`ceiling` parameters from `delegate`/`delegate_batch` (all ~10 call sites).
- `LocalDelegateResult`: `cost`, `cost_uncapped`, `balance` fields (and their JSON rendering). Tokens/latency/model/tool-calls/reasoning — the real measurements — remain. Old stored delegation records with cost fields still deserialize (serde ignores unknown fields; no migration needed).
- The three tools `swarm_fund_local`, `swarm_balance_local`, `swarm_local_history` (`ledger_tools.rs` deleted; router, TOOL_NAMES via build.rs, and the tool-count pin 85→82 updated).
- `credits_authorized` from all LOCAL request types (`DelegateLocalRequest`, `FanoutEntry`, `PipelineStep`, `A2aSendRequest`, `A2aBroadcastRequest`, `PlanDelegation`, `EvalAgentTask`). The CLOUD (ABW) request types keep theirs — the cloud credit system is real and consent-gated.
- Kata-kanban's `HKASK_ABW_MAX_CREDITS` read + credits/cost/balance theater in the spawn path; the spawn note now records tokens/model/latency only.
- `AgentStatsStore.total_cost_credits` (cost param dropped from `record_success`).
- The bridge's `HKASK_SWARM_LEDGER_PATH` emission + 3 allowlist entries + the settings-reference row; the A2A HTTP gateway's `max_credits_per_dispatch` parameter chain.
- The fund/balance/history round-trip test and the funding-gesture pin (replaced by `local_delegation_has_no_budget`); the panel's Steer-prompt ledger paragraph and its pre-funding assertions; `KANBAN_TOOLS` count pin 26→25 (fallout of the parallel session's rJoule tool removal, fixed here because it broke the panel build).
- Docs: swarm.md (tool table, local-mode descriptions, algedonic section, consent-gate local note, env table), the diataxis swarm_system set (explanation/reference/tutorial/how-to — including the class diagram, sequence diagram, and "why no funding gate" → "no budget"), kask_bridge reference, diataxis INDEX.

**What stayed (and why):** the CLOUD spend gate untouched (T05's reservation/settlement semantics — ABW credits are real spend); `max_credits_per_dispatch` in `SwarmConfig` (cloud ceiling); fermi's cloud spend-honesty fields; tokens/latency/model measurements; `AgentStatsStore` (real per-agent execution stats).

**Net-LOC accounting:** the two teardown commits (`29d3330507`, `a4f82b3deb`) — **41 files in the budget scope: +631 / −2,052 (net −1,421)**; the repo-wide commits total +1,152/−2,668 including the parallel session's unrelated grounding-verify edits. First net-negative slice of the program.

**Verification (2026-09-08):** `cargo test --offline --locked -p hkask-mcp-swarm -p hkask-mcp-kata-kanban -p swarm_panel -p kask_bridge -j 4` — 132 + 46/21/1 + 69 + allowlist suites, **0 failed**; `HKASK_BUILD_JOBS=4 CARGO_NET_OFFLINE=true ./script/clippy --locked -p hkask-mcp-swarm -p hkask-mcp-kata-kanban -p swarm_panel -p kask_bridge` clean; rustfmt clean on every touched file; `git diff --check` clean. The kata-kanban spawn path and T06 replay tests pass unchanged (the replay protection is orthogonal to the budget removal).

### C2 — Consolidate the T04–T06 additions — CLOSED 2026-09-09

**Core (committed `c38c26b741`):** the `Settlement` type is now THE authorization type. The `HireAuthorization`/`DelegateAuthorization` newtype wrappers (two structs × three pure-delegation methods each) and the `DispatchSettlement` trait (2 impls × 3 methods, for two call sites) are deleted; `settle_dispatch_failure` is concrete over `Option<Settlement>`; `authorize_hire`/`authorize_delegate`/`authorize_curate`/`complete_*` and `CuratorSession` carry `Settlement` directly (its `release`/`hold`/`held_description` are `pub(crate)`). Three delegation layers became zero. Behavior unchanged (the T05 settlement tests pin it). Net −82 across the three files.

**Leftovers closed (2026-09-09, committed `017ec0d210`):**

- **(a) Test-fixture dedup — done.** The duplicated `test_client` + `sqlite_store` helpers are extracted into the `test_http` fixture module in `abw_client.rs`; the `spend_gate.rs` and `cloud_swarm/curator.rs` test modules import them and drop their local copies plus the now-unused `SwarmConfig` imports. Net −13 (+30/−1 shared module vs 2×~20-line local copies). Swarm suite 132 green, same count — behavior-preserving.
- **(b) `DistillationCursor::merge` eviction block — assessed, KEPT.** The current form (build the merged map preserving first-seen → sort by age → drain evicted with a per-thread warn → rebuild) is the direct statement of "bounded set, evict longest-pending, warn naming each." Both candidate replacements add machinery rather than remove it: a VecDeque keyed by first-seen needs dedup-on-insert and O(n) resolved-thread removal, because `merge` is a full-set reconciliation each pass (the new pass's pending set replaces the old, preserving first-seen for survivors) — a HashMap expresses that directly; an evict-oldest scan loop (`while len > MAX { min_by_key; remove; warn }`) saves ~6 lines but needs a key clone per eviction to work around the borrow checker (a map cannot be mutated while its items are borrowed) plus an `unwrap` the bound only implies. The T04 tests (`pending_threads_are_bounded_with_explicit_eviction`: bounded set, newest survives, survivors distilled) pin the behavior either way. Changing it would be churn dressed as simplification — the teardown ruling cuts the other way too.
- **(c) T06 response-format residue — confirmed clean.** The kata-kanban spawn path has zero credits/cost mentions (grep over `hkask_mcp_kata_kanban.rs` spawn/note/comment lines: empty); the spawn note records tokens/model/latency only (C1); no `credits_authorized` survives on any LOCAL request type — only the CLOUD (ABW) types carry it, which is the ratified keep.

### C3 — Repo-wide residue sweep — DONE 2026-09-09

Sweep executed over kask/ code, docs, templates, scripts, and comments, plus `.agents/skills` and the task records. Two tranches: tranche 1 (templates, SKILL.md, docs, kata-kanban comments, the C2 fixture dedup) was committed by the external process as `017ec0d210` (30 files, +83/−145, net −62); tranche 2 (swarm-server dead code + comment surgery + kata-kanban README) was in the worktree, validated green, at report time (11 files, +53/−83, net −30).

**Dead code the sweep surfaced (C1 leftovers, deleted):** `FundLocalRequest`, `BalanceLocalRequest`, and `LocalHistoryRequest` in `request_types.rs` — the request types of the three ledger tools C1 deleted, zero references anywhere; and the `LocalSwarmError::Ledger` variant plus its match arm in `map_local_swarm_error` — no constructor since the ledger died.

**Stale-comment surgery:** the post-rJoule "ledger records spend rather than authorizing it" framing survived C1 in ~20 sites and cited a deleted mechanism as the reason for live behavior. Fixed in: `local_runtime.rs` (struct doc, event-store path, test-constructor doc), `agent_executor.rs` (the "does NOT debit the ledger" ADR framing → "returns raw output only; the runtime builds the measured result"), `a2a_tools.rs` + `local_tools.rs` (every "ledger TOCTOU"/"single-writer" dispatch rationale → the live one-delegation-at-a-time discipline; the fanout `parallel` flag's rationale → concurrent inference, sequential side effects), `a2a_http.rs` (doubly stale — cited both the deleted ledger AND the deleted per-dispatch ceiling; the agent-card allowlist is the whole defence), `local_swarms.rs`, `cloud_swarm_tools.rs` (commented-out local-mode gate lines removed), `build.rs` (router list still named the deleted `ledger_tools.rs`), and kata-kanban's Cargo.toml comment, server doc comments, and README tool rows.

**Templates/SKILL.md/docs:** kanban-task-management (dead `rjoule_budget`/`rjoule_remaining` params and fields in monitor/populate/configure-spawn/verify-completion); tdd (the running example re-anchored from the removed `energy_budget::can_proceed(rJoules)` to the LIVE `hkask_regulation::energy::CallCap::can_proceed` — a call-count cap); capabilities-reasoner; gemba-walk (rJoule-cap examples → call caps); prompt-enhance; swarm-intelligence (`swarm-sense`'s `LocalDelegateResult` field list corrected to the live fields — the cost/cost_uncapped/balance explanation deleted; `swarm-decide`'s local branch rewritten to the truth — no local credit cost, tokens/latency are the cost signal — and the cloud branch relabeled "Credit awareness"); gpa-evolution (cost fields → tokens/latency); pragmatic-cybernetics (VSM S3 + variety table → call caps); skill-maintenance (the dead `rjoule:` manifest-block requirements deleted from build/validate/translate — no manifest in the registry carries one and no code parses one; validate checks E2/E6 removed, surviving check IDs stable since E10–E14 are cross-referenced); hkask-types README (dead `RJoule` type row); `docs/diagrams/kanban.md` (dead budget-exhaustion state transitions); `.agents/skills` kanban-task-management + gpa-evolution SKILL.md; the reduct scaffold's cost-model row.

**Negative claim (grep-able, 2026-09-09):** `grep -rni "rjoule" kask/` returns exactly two hits, both intentional — the training server's serde compat-alias decision record (`estimated_cost_urj` alias keeps stored job records deserializable; load-bearing, do not remove) and the kata-kanban resurrection pin test. Remaining `ledger` mentions are live or out-of-scope concepts: `RegulationLedger` (the regulation event store), the portfolio transaction-ledger, the kata-kanban "No ledger" decision records and unfunded-ledger pin, and a storage-macro doc example. Remaining `budget` mentions are live concepts (Lisp step budgets, LoRA rank/memory gates, `EnergyBudgetExceeded` runaway-loop breaker, corpus embedding budget). No dead rJoule spec-doc links survive — the ledger-crate README died with the crate, and no kask/ doc links to a rJoule spec.

**Count-drift check:** the swarm 82 pin and the panel's KANBAN_TOOLS 25 pin both pass — no tools were removed this tranche (only dead request types whose tools died in C1).

**Net-LOC (Phase E cumulative through C3):** C1 −1,421; C2 core −82; C2 leftovers −13; C3 −79 (tranche 1 −49 within `017ec0d210` after excluding the fixture dedup, tranche 2 −30 in the worktree). **Through C4: net −3,738** (C4 −2,143, below).

**Verification (2026-09-09):** `cargo test --offline --locked -p hkask-mcp-swarm -p hkask-mcp-kata-kanban -j 8` → 132 + 46/21/1, **0 failed**; `-p swarm_panel` → 69, 0 failed (KANBAN_TOOLS pin); `HKASK_BUILD_JOBS=8 CARGO_NET_OFFLINE=true ./script/clippy --locked -p hkask-mcp-swarm -p hkask-mcp-kata-kanban` exit 0, zero warnings; `cargo fmt --check -p hkask-mcp-swarm -p hkask-mcp-kata-kanban` clean. (An external in-flight change to `hkask-regulation`/`hkask-mcp-curator` — a memory-citation round-trip test — appeared in the worktree during this session; it is outside this program's scope, not counted here, and not validated by these runs.)

**Post-restart confirmation (2026-09-09, after the operator's rebuild):** tranche 2 landed in `03730ff057` (alongside the external curator work; that commit's message says "fund/balance tools" but the tools died in C1's `29d3330507` — this commit removed the orphaned request types and the `Ledger` error variant). Key deletions verified absent at HEAD; the negative claim re-verified on HEAD; suites re-run green on HEAD (132 + 46/21/1 + 69, 0 failed); live read-only probes (`swarm_list_local_agents`, `kanban_board_list`) confirm the restarted servers run the new build.

**Addendum — the stored-data class (2026-09-09):** the live probe surfaced a residue class the code/docs grep could not see: the three in-repo local agent cards (`agents/local/curated/{local_critic,local_extractor,local_narrator}/agent_card.json`) described the deleted workflow ("fund → delegate → debit") in their `description` fields, served verbatim by `swarm_list_local_agents`. Fixed (the parenthetical deleted; delegation is the only remaining step). A repo-wide `.json` sweep then found no other budget-concept residue (only upstream npm-schema "fund" strings in `crates/json_schema_store`). The running registry holds cards from startup, so the corrected descriptions surface on the next server restart or card write — no restart required for a description string.

### C4 — Dead-surface removal (operator directive 2026-09-09: "every variable and component should be load bearing")

**Method:** compiler dead-code warnings (the `allow(dead_code)` baseline was masking them), `cargo machete` over our crates, and closure-mapping every flagged item to its callers. The `allow(dead_code)` inventory in our scope was the entry point: 22 allows across 6 files, every one masking either an unread serde field, a write-only struct, or an unreachable feature.

**The big find — the swarm panel's roster drill-down was structurally unreachable:** `SwarmDetailView` (the swarm detail/roster view) could never open. Its only constructor (`open_swarm_detail`) was called exclusively from refresh paths that clone an *existing* detail; the detail starts `None`; and the browse card's Edit button routes to the Compose view (which explicitly *closes* the detail). The compose flow replaced this view, and a struct-level `allow(dead_code)` on `SwarmCard` hid the corpse. **Consequence found live:** the card's Delete button was silently broken — it staged a confirmation that rendered only inside the unreachable view, so the delete never fired. **Repair:** completed the strangler-fig migration — deleted the dead path (~1,300 lines: `detail.rs` 802-line renderer, `SwarmDetailView`, `PendingActionsView`, `SwarmRosterAgent`, `open/close_swarm_detail`, `fire_agent`, `add_agent_to_swarm`, `remove_agent_from_swarm`, `request_remove_agent` + the `RemoveAgent` variant, `clone_local_swarm`, `push_local_swarm_to_cloud`, `pull_cloud_swarm_to_local`, `clone_swarm_to_compose`, the metadata-edit methods, the pending-actions accept/reject/refresh trio, `parse_swarm_roster`, `parse_pending_actions`, `staleness_chip`, the write-only stats-parse chain `AgentInfo.execution_stats`/`updated_at` → `ExecutionStats`/`LocalExecStats`, and the cascade `AgentSource::badge`/`label`, `LocalAgentInfo.accepts`/`produces`, `LocalSwarmInfo.members`) — and re-homed the destructive confirmation as a minimal banner in the browse list (where the run-status strip already renders), so the Delete button works again. `save_swarm_metadata` keeps only its compose branch (the detail branch read editors that no longer exist).

**The rest of the tranche:** `in_memory_port_for_tests` in kask_bridge (zero callers — its doc comment claimed test modules shared it; they use the private `in_memory_port` instead); `FmpEntry.year` in companies (unread — the requested year is authoritative); 10 unread serde fields in the prediction-markets fetch bins (serde ignores unknown JSON fields — the fields were pure noise); 11 unused Cargo deps across the three kask panels (`agent`, `agent_ui` ×3, plus portfolio's `editor`, `hkask_tool_invoker`, `remote_connection`, `serde`, `serde_json` — scaffolding copied when the panels were forked from swarm_panel) and portfolio's dead `[features]` block; `chrono` from swarm_panel (staleness_chip was its only user). The allow baseline refreshed: 6 files dropped to zero allows.

**Spec-loss note (operator visibility):** the roster drill-down deletion is completing a replacement, not removing a capability — the Compose view is the detail UX (the card's Edit button's tooltip says so), and the delete-with-confirmation capability is preserved via the re-homed banner. Cloud fire-from-panel (`swarm_fire` via the panel) was reachable only through the dead path and is gone from the panel; the `swarm_fire` MCP tool remains (Steer chat path). The panel's TOOL_NAMES pin tests still pin the server tool list.

**Net-LOC: −2,143** (169 insertions / 2,312 deletions, 19 files including Cargo.lock and the baseline).

**Verification (2026-09-09):** `cargo test` — swarm_panel **61** (8 tests died with the dead code), kanban_panel 22, portfolio_panel 2, kask_bridge 66, companies 25+48, prediction-markets 182+2, **0 failed**; `./script/clippy` on all six crates exit 0, zero warnings; rustfmt clean; `check-no-new-dead-code-allows.sh` passes with the refreshed baseline; `cargo machete` clean on our crates; full-repo symbol sweep over every removed identifier (code + docs) clean; **`cargo check -p zed` exit 0** (the panels are zed deps). Two machete flags were deferred at C4 report time (media_panel as the parallel session's scope; zed's `context_server` as "upstream surface") — both deferrals were wrong and are corrected in C5 below (the operator ordered them fixed 2026-09-09).

### C5 — The deferred flags, fixed (operator directive 2026-09-09: "don't flag and then not do anything")

The operator ordered the two reported-not-fixed machete flags closed. Both deferrals were mistakes — one wrong about scope, one wrong about ownership:

- **`media_panel` — 5 unused deps + a dead features block, removed.** `agent`, `agent_ui`, `editor`, `remote_connection`, `serde` (each verified zero `use`/path hits across src; the earlier `serde` "hit" was `use serde_json` matching the loose pattern). The `[features] test-support = ["workspace/test-support", "remote_connection/test-support"]` block was scaffolding copied from swarm_panel: zero `cfg(feature = "test-support")` usage in src, no tests dir, nothing in CI or any manifest enables `media_panel/test-support` — and it had to go with the `remote_connection` dep it references. Recorded here per the parallel-session constraint ("record any such fix"); the media files were uncommitted-work-free at edit time.
- **`crates/zed`'s `context_server` — removed; it was OUR orphan, not upstream's.** Archaeology: upstream added the dep in #20250 and REMOVED it from `crates/zed` in #21083 (`1cfcdfa7ac`); kask commit `1e05483a63` (models settings page) re-added it for `context_server::ContextServerCommand` usage; `6c21eeabfb` later deleted that usage and left the dep. Zero path-usage anywhere in `crates/zed/` today (the grep hits are string literals and variable names). Removing it RESTORES upstream's line set — divergence-reducing, no new D-seam required. My C4 classification ("upstream surface, per the machete scoping rule") was wrong and is corrected.

**New issue found and fixed while validating — swarm_panel's `--all-features` landmine:** plain `cargo check --all-features -p swarm_panel` failed E0004 (`RemoteConnectionOptions::Mock` not covered in `remote_connection.rs`). Root cause: the "Feature-propagation-only dev-dep" design — `remote_connection` was a DEV-dep, so the `test-support` feature's `remote_connection/test-support` request reached only the dev unit, never the normal unit a lib-only check builds; meanwhile `workspace/test-support` (a normal dep) turned `remote/test-support` on in that same unit, exposing the Mock variant without its match arm. Verified PRE-EXISTING (the pre-C4 manifest fails identically — 2 errors) and invisible to every gate: `./script/clippy` always adds `--all-targets --all-features` (pinned by `check-build-profile.sh` D52), and `--all-targets` unifies the dev request (0 errors). The `agent` crate's variant of the trick is self-consistent (hardcoded dev-dep features; plain `--all-features -p agent` passes) — untouched. **Fix:** `remote_connection` moved from `[dev-dependencies]` to `[dependencies]` in swarm_panel (comment rewritten with the mechanism), so the feature request reaches the normal unit; the feature still gates when test-support is on, so `cargo test -p swarm_panel` without `--all-features` leaks nothing. The machete-ignore entry stays (the dep remains unused in source, by design).

**What remains flagged and why it stays:** the other 26 machete findings are upstream Zed crates (`tracing`/`component` false positives from macro-activated usage — e.g. `mermaid_render`'s only "tracing" use is `#[ztracing::instrument]`; upstream manages its own machete config, per #62643). The `.rules` machete scoping rule exists to avoid editing ~25 upstream Cargo.tomls for rebase friction; those findings belong to upstream CI.

**Net-LOC:** −16 manifest lines (media_panel −10, zed −1, swarm_panel −5 net after the comment rewrite) plus Cargo.lock shrinkage.

**Verification (2026-09-09, complete — the operator killed the in-flight external release build to free the lock):** `cargo check --all-features -p swarm_panel` → **0 errors (was 2 — the fix)**; `cargo check --all-features` across all four kask panels → 0 errors; `cargo test -p swarm_panel` → 61 passed, 0 failed; `./script/clippy -p swarm_panel -p media_panel -p zed` (the wrapper's `--all-targets --all-features`) exit 0, zero warnings — this is the heavyweight validation of the `zed` dep removal; `cargo fmt --check` clean; `cargo machete` final state: **zero findings in our scope** — `media_panel` and `zed` cleared, `swarm_panel` clean under its documented ignore; the 24 remaining findings are all upstream Zed crates (the `tracing`/`component` macro-activated false-positive class).

## Phase D — Specification-truth repairs (operator ruling 2026-09-07)

**Continuation:** Phase D implementation, automated checks, and release-build commands are complete; the commands reused current artifacts. Read the release/runtime evidence above and [the continuation](kask-reliability-final-verification-continuation.md) for the still-pending live functionality checks and remaining T04–T15 work. The continuation's unstarted-build statement is superseded by this evidence. The older [Phase D prompt](kask-phase-d-continuation-prompt.md) is historical and includes claims corrected by this phase's evidence. Keep one build at a time and outputs trimmed.

**Operator ruling (recorded verbatim intent):** the three items surfaced by the 2026-09-07 dead-code sweep are **not open questions**. They are requirements the code pretends to meet: "memory life should map to the days in the setting — the code that fails to do this is a lie and deception"; "memory consolidation is required"; the salience failure is "another deception". "The problem is not the requirements and specifications — the problem is the shit code, and that is what we are trying to fix." These tasks build the code to meet the specifications. No item in this phase is a policy gate.

**One retraction, entered into the record:** the sweep's consolidation flag was WRONG — a grep artifact (output truncated before the bridge hits). Consolidation IS built and wired: `zed/src/main.rs:1764` starts the production timer, which fires `fire_curator_consolidation_pass` (`kask_bridge/src/memory.rs:283`) → `MemoryConsolidator::consolidate` (`memory.rs:439`) with `confidence_floor` from `KaskMemorySettings` (`main.rs:1751`); ingestion rebuilds the consolidator after a store heal (`ingest.rs:120-128`); the fire callback is tested (`memory.rs:1978`). The component exists and is called, but the original inference that it fires was wrong. **Subsequent correction, 2026-09-08:** the production timer's `last=None` + `fire_when_no_last=false` gate never fires. The callback-only test did not cover scheduling. Historical RED and the interval-native repair are recorded above; wiring presence is not behavioral proof.

### T16 — Wire `kask.memory.memory_life_days` into actual decay

**Scope:** M; trust; `kask-bridge`, `hkask-mcp-curator`, settings emission. **Depends on:** none. **Policy: resolved by operator ruling above.**

**The deception, anatomized (all verified 2026-09-07):**
- The setting exists and defaults correctly: `KaskMemorySettings.memory_life_days` (`kask_bridge/src/settings.rs:238`), default from `MemoryStore::default_memory_life_days()` (`settings.rs:268`).
- The regulation sensor REPORTS it: `RealMemoryPort::memory_life_days()` (`memory.rs:547`) reads the setting and feeds the regulation `MemorySource` sensor (`memory.rs:1018`).
- The store that actually decays IGNORES it: `MemoryStore::with_memory_life_days` (`hkask-memory/src/memory_store.rs:198`) has **zero callers**; the bridge's curator store (`kask_bridge/src/memory/curator_stores.rs:174-225`) and the curator MCP server's store (`hkask-mcp-curator/src/hkask_mcp_curator.rs`, `open_curator_stores` ≈ :1931) both construct with the hard-coded `DEFAULT_MEMORY_LIFE_DAYS` = 180.
- The documented env knob is fictional: `HKASK_MEMORY_LIFE_DAYS` appears only in doc comments (`memory_store.rs:132-139`, `hkask-regulation/src/loops/signals.rs:29` — "Configurable via HKASK_MEMORY_LIFE_DAYS") — emitted nowhere, read nowhere.

Net effect: the operator's setting changes what regulation MONITORS while the monitored behavior stays constant at 180. The sensor reports fiction.

**Build steps:**
1. Bridge (in-process): pass `kask_settings.memory.memory_life_days` into `RealMemoryPort::new` (call site `zed/src/main.rs:1744`), thread to `open_curator_store`, apply via `with_memory_life_days` at the `MemoryStore` construction (`curator_stores.rs:225`).
2. Curator MCP server (separate process): extend `emit_curator_distillation_env` (`kask_bridge/src/mcp_env.rs:62`) to also emit `HKASK_MEMORY_LIFE_DAYS` from `KaskMemorySettings`; add the env name to the per-server allowlist (`kask_servers` registry — same line as `HKASK_MEMORY_DISTILLATION_CADENCE_SECS`, `mcp_servers.rs:218`); in the curator server's `open_curator_stores`, read it with the house rule (malformed value → `warn!` naming the value → default 180) and apply via `with_memory_life_days` on BOTH construction paths (with and without embeddings).
3. Sensor truth: after wiring, the regulation sensor value and the store's applied value must be the same value — derive the sensor reading from the applied store state (or pin equality in a test), so monitoring can never again diverge from behavior.

**Acceptance:**
- Setting 30 days → `store.memory_life_days() == 30.0` on the bridge's curator store AND the curator MCP server's store; both construction paths covered.
- Decay behavior actually changes: recall-time confidence decay uses S=30 (a seeded h_mem with `recalled_at` 15 days ago decays measurably more under S=30 than S=180 — deterministic via the Wozniak-Gorzelanczyk formula).
- Malformed env value (`"abc"`) warns naming the value and falls back to 180 — never a silent default.
- Emission and allowlist stay aligned (the `research_allowlist_matches_actual_reads` house pattern).
- The regulation sensor and the store agree.

**Verification (RED first):** a test constructing the bridge store with `memory_life_days: 30.0` asserting `store.memory_life_days() == 30.0` fails today (180); same for the curator server's `open_curator_stores` under `HKASK_MEMORY_LIFE_DAYS=30`. Then GREEN, plus the behavioral decay test and the malformed-value test. Affected suites: kask-bridge memory tests, curator server lib tests, settings/allowlist tests.
**Refused shortcut:** making the SENSOR read the setting while the store keeps the default (the current lie); adding a new setting name; wiring one construction path and not the other.

### T17 — Consolidation: retraction + production-timer pin

**Scope:** S; production scheduling repair and timer tests. **Depends on:** none.

**Implemented:** the operator-directed interval-native timer skips the immediate tick, then fires a real pass every `max(configured cadence, 60s)`. Timestamp/flag machinery and test-only consolidation entry were removed. The old one-hour polling cap cannot be retained when every tick performs a pass: it shortened longer configured cadences. Paused-time tests pin first firing, the two-hour cadence, disabled cadence, real confidence pruning, and ingestion independence. Historical RED confirms the original scheduling defect; the full bridge suite is green. App compilation and scoped lint subsequently passed (evidence above).
**Observation (not a defect, recorded for the spec's next revision):** consolidation's budget-prune phase (`consolidation_service.rs:39-68`) is count-based, documented as deliberate design ("confidence-floor cleanup plus budget pruning only", spec §5; the budget as Ashby attenuator, `curator_stores.rs:226-230`). The 2026-09-04 "never count-based" ruling governs FORGETTING (turn deletion), not consolidation's confidence-ranked pruning; if the operator wants consolidation pruned of its budget leg too, that is a separate decision — the current documentation is internally consistent.

### T18 — Wire method signals and keyword overlap into the pipelines they were designed for

**Scope:** M; domain; `hkask-mcp-corpus`, `kask-bridge`. **Depends on:** none.

**The deception, anatomized:** `hkask-memory/src/salience.rs`'s method-signals half — `compute_method_signals` (`:95`), `MethodSignals` (`:26`), `DeclaredMethod::matches` (`:570`), `keyword_overlap` — was built for the condenser (commit `2b651fc956`, "Add method signals and centroid support"); the condenser crate was later removed (`4466a5fbb3`), orphaning the feature while the module doc still claims consumers ("Used by EmbedService at embed time (budget gating), by the style synthesizer at query time, and by chat recall (episode ranking via keyword overlap)") that were never (re)built. Only `tag_entities` + `compute_salience_batch` are wired (corpus `tools/tagging/ops.rs:114-133`). The 5W1H design — the "how" dimension — is live spec surface: `corpus_tag_chunks` advertises 5W1H annotation (`kask/docs/reference/mcp-servers/corpus.md:56`); the ontology-bridge invariant is "nothing is ever untagged" (`kask/docs/reference/ontology-bridge.md:43`).

**Build steps:**
1. **Tag time (the 5W1H "how" dimension):** in the `corpus_tag_chunks` pipeline (`corpus/tools/tagging/ops.rs`), compute `compute_method_signals(chunk_text)` per chunk — zero LLM cost, by design — and store the signals in the chunk's ontology metadata so the "how" dimension is real on every tagged chunk, not just the who/what that EntityTags carries today.
2. **Compose time (declared-method matching):** the style composition request surface (`corpus/compose.rs` — the existing `salience_min`/`salience_top_k` parameters) gains declared-method threshold parameters; candidate passages filter through `DeclaredMethod::matches(&stored_signals)` so a composition can select passages whose methods match declared thresholds. Follow the existing request-parameter pattern — no new tool, no new settings surface.
3. **Chat recall (episode ranking):** wire `keyword_overlap` into the bridge's episode-ranking seam in the recall path (`kask_bridge/src/memory.rs` recall ranking), replacing/augmenting the current relevance×confidence ranking for keyword-scored episodes per the module's stated design.
4. **Truth in docs:** rewrite the salience module doc to name the actual wired consumers (corpus tagging, corpus compose, bridge episode ranking) — the stale "EmbedService" claim dies with the wiring.

**Acceptance:**
- A tagged chunk's ontology carries computed method signals (fixture text with known parataxis/adjective density → signals within expected ranges).
- A composition with a declared threshold selects only matching passages; without thresholds, behavior is unchanged.
- Episode ranking uses keyword overlap (fixture episodes + query → order changes accordingly).
- No stale consumer claims remain in the salience module doc.

**Verification (RED first):** tests for each acceptance line fail today (signals absent from ontology; no threshold filtering; ranking ignores keyword overlap). Affected suites: corpus lib + tool-behavior, kask-bridge memory tests.
**Refused shortcut:** deleting the unwired half (forbidden by the operator ruling); wiring without behavioral tests; leaving the stale doc in place.

### T19 — MCP servers die with the zed-kask session (operator directive 2026-09-07)

**Scope:** S–M; trust/lifecycle; `hkask-mcp`, `crates/zed/src/main.rs`, D51. Runtime unit/integration tests, application check and lint are green; live editor-quit confirmation is the operator checkpoint. **Depends on:** none.

**Initial diagnosis (superseded in part by the real-process RED below):** the kill chain existed all along — children spawn with `kill_on_drop(true)` (`hkask-mcp/src/runtime.rs`, `start_connection`), and `StdioTransport::Drop` kills a still-running child (`crates/context_server/src/transport/stdio_transport.rs:229-251`) — but **app exit never runs Rust destructors**, so nothing ever dropped the transports at quit and every managed MCP server child orphaned and kept running after zed-kask shutdown. The settings-unload path (`sync_kask_mcp_runtime_servers` → `stop_server`) works; only the session-end call was missing — the deception the operator named: "the mcp servers are supposed to be killed by the shutdown of zed-kask."

**Implemented and corrected:** the original lifecycle-locked `shutdown_all` survives; the duplicate union-loop definition was removed. A real-process regression disproved the handoff's "only the quit hook is missing" diagnosis: rmcp owns asynchronous graceful cleanup, so clearing maps does not await child death. The runtime now owns each spawned child before handshake, gives rmcp only its pipes, and tracks cancellable kill-and-reap tasks. Stop and shutdown await those tasks; terminal shutdown rejects queued starts. `wire_kask_mcp_shutdown` registers `on_app_quit`, runs shutdown on the Tokio handle, and `.detach()` retains the subscription. The fn-pointer pin exists; production quit wiring compiled in the full Zed check. The Zed unit-test pin itself and live editor quit were not separately executed.

**Acceptance:** on quit, every managed server process is gone (no `hkask` processes survive the editor); a mid-retry server's spec/token is torn down too (no resurrection post-quit); the runtime's stop path is exercised by the quit hook, not only by settings changes.
**Refused shortcut:** relying on pipe-EOF graceful exits (a blocked server ignores EOF); PDEATHSIG (needs `unsafe`, forbidden by crate policy); only killing live connections (a mid-retry server would resurrect).

**Checkpoint D:** T16–T19 regressions, affected crate suites/checks/lints, residue review, operator review of the retraction record.

## Follow-up queue: elaborated at Checkpoint C (2026-09-09, ratified) — awaiting operator scheduling

Each entry below is the recovered-spec elaboration (defect verified against current code 2026-09-09, fix approach, acceptance, verification, refused shortcut). Execution still requires operator scheduling; the three-criterion expansion happens in the task section when executed.

### T09 — Passage deletion ownership — VERIFIED 2026-09-09 (Group 1)

**Outcome:** the deletion machinery was already correct (landed with the 2026-09-09 orphan-cleanup ruling — passage-scoped deletion is transactional across the vector and metadata rows); the missing piece was the evidence, now captured. No defect — this is evidence-capture, honestly recorded as such (no RED to observe).

**Evidence:** `passage_deletion_removes_only_the_named_passage` — seeds one entity with three passages (distinct one-hot vectors), deletes the middle passage, asserts: exactly 1 row deleted, count 3→2, the deleted passage is not retrievable via KNN, both siblings are the nearest match for their own vectors, the entity's h_mems survive, and the orphan sweep removes nothing (survivors with a live entity are not orphans). Control: `passage_deletion_spares_null_passage_and_unlisted_entities` — a NULL-passage legacy row survives a passage-listed deletion, and a passage listed under a different entity deletes nothing. The T09-class prefix-collision observation is pinned at the distillation watermark read site (`distillation.rs:452`): the read is entity-PREFIX, safe only because production thread ids are UUIDs; a future non-UUID id source must switch to exact-match first.

**Verification (2026-09-09):** hkask-memory 37 passed (35 + 2 new), 0 failed; curator suite green (comment-only edit); clippy exit 0, zero warnings; rustfmt clean.

### T10 — Exact harness comparison — VERIFIED 2026-09-09 (Group 1)

**Defect (confirmed, RED observed):** both extraction sides failed. `before` took the FIRST metric value at-or-before the detection position — in a 0.4→0.9→0.6 history with the detector at 0.9, before=0.4, and the impact verdict could INVERT (0.4→0.6 reads "improved" where the real detection pair 0.9→0.6 degraded). `after` used `rfind` over ALL events — a trailing non-metric event (a verdict) made after=None, suppressing the comparison entirely. RED: `metric_before_and_after_preserves_the_detection_pair` failed with `None` where `Some((0.9, 0.6))` belongs.

**Fix:** both sides now extract from METRIC-VALUED events only, taking the LATEST on each side of the detection point — `before` is the value the detector saw (latest at-or-before), `after` is the latest measurement since (trailing verdicts cannot suppress it). The spec's "recover both summary identities" note was considered: the event model carries identity as (position, kind, metric-valued payload) — the metric filter IS the identity filter for metric events; a tag-based detector/verification identity redesign would add machinery without changing any acceptance outcome, so it was rejected under the teardown discipline.

**Evidence:** `metric_before_and_after_preserves_the_detection_pair` (0.4→0.9→verdict→0.6→verdict → exactly (0.9, 0.6)); `metric_before_and_after_skips_other_metric_events` (a latency event between is never picked up as the pass_rate pair); the two pre-existing tests unchanged and green (the two-event case and the no-after absence case).

**Verification (2026-09-09):** kask_bridge 187 passed (185 + 2 new), 0 failed; hkask-memory 37; curator green; clippy exit 0, zero warnings; rustfmt clean.

### T11 — Market-identity calibration — VERIFIED 2026-09-09 (Group 2)

**Defect (confirmed, RED observed):** the rescan guard `contains` deduped on `(probability, outcome)` — no market identity. Five DISTINCT markets at 0.9/no yielded ONE sample (RED: `distinct_markets_with_identical_prices_count_independently` failed — m2 was a "duplicate" of m1); the Brier loop under-counted exactly where discrimination matters.

**Fix:** `ResolvedObservation` gains `market_key: Option<String>` (serde-defaulted — legacy journal lines load as `None`); `contains` dedups on MARKET IDENTITY — same key → duplicate (a market resolves once; identity matches even if the re-scanned price differs), different keys → never duplicates, `None` → never a duplicate (fabricating an identity the row does not carry is the refused shortcut). The journal row carries the field (additive — legacy lines load unchanged and are preserved as-is; the operator's legacy-migration gate was not triggered). Both providers thread the key at the observation construction site (Kalshi `ticker`, Polymarket Gamma `id`); the manual `market_record_resolution` tool records identity-less (unchanged — it never went through the guard).

**Evidence:** five distinct 0.9/no markets → five samples, Brier 0.81, sample_size 5; rescans of all five → zero new; legacy identity-less rows never dedup; the updated `contains_guards_idempotent_ingest` pins the identity contract (same key + different price is still a duplicate; different key + same price is not).

**Verification (2026-09-09):** RED observed (3 tests failed against the old logic); GREEN: prediction-markets **50 passed** (47 + 3 new/updated), 0 failed; clippy exit 0; rustfmt clean.

### T12 — Cap-reset evidence — VERIFIED 2026-09-09 (Group 2)

**Defect (confirmed, RED observed):** `act()` called `reset_all_caps()` BEFORE the exhaustion check — post-reset, remaining==ceiling, so `remaining == 0` was never observable and the E04 exhaustion alert was dead code (RED: zero alerts for a fully-exhausted agent).

**Fix:** the exhaustion snapshot is captured BEFORE the reset (charges land between ticks via metered dispatches, so the pre-reset read is the only moment exhaustion is observable); the reset then replenishes; the alert block consumes the captured set. Design note (pinned by the test): the exhaustion alert is transient — `escalated: false`, no recovery signal — so it never enters the reviewable queue and the auto-resolve machinery has nothing to credit the automatic reset with ("reset alone earns no advice-progress credit" is structural, not just tested).

**Evidence:** `cap_exhaustion_is_detected_before_the_reset_replenishes` — an agent charged to zero produces exactly one alert naming it; the reset still replenishes (cap back at ceiling); a second `act()` with no new charges does not re-alert; the escalation queue receives nothing (transient routing pinned) and nothing auto-resolves (no advice credit).

**Verification (2026-09-09):** RED observed (0 alerts); GREEN: hkask-regulation **72 passed** (71 + 1 new), 0 failed; clippy exit 0 (one `await_holding_lock` false-positive on an explicit `drop()` resolved by block scoping — the lint's analysis does not track the drop call); rustfmt clean.

**Group 2 net-LOC:** +26/−25 in cycle.rs (the reorder) + the calibration identity threading (~+60 across calibration.rs/providers) + tests.

### T13 — Retrain finalization — VERIFIED 2026-09-09 (Group 3)

**Seam recovered first (per the dependency):** the adapter-registration + A/B block extracted from `training_status` into `finalize_completed_job(job_id, manifest, &mut result)` — testable with a fixture manifest, no live HuggingFace fetch. Behavior unchanged by the extraction (all 15 pre-existing tests green).

**Defect (confirmed, RED observed):** `uuid::Uuid::parse_str(&job_id).unwrap_or_default()` — a malformed job id silently became the nil UUID, the pre-registration lookup silently missed, and the flow proceeded to register under the malformed id. RED: `malformed_job_id_is_rejected_not_silently_registered` failed (the call returned Ok, registering nothing but silently skipping the check). **Fix:** a malformed job id is a typed `invalid_argument` error naming the id.

**Evidence:** the malformed-id rejection (RED→GREEN); `repeated_finalization_is_idempotent_with_durable_metrics` — first poll registers with the manifest's durable loss (0.42) and base_model, second poll finds it pre-registered ("Already registered"), no duplicate (the get_by_id pre-check makes the poll idempotent — now pinned); `missing_manifest_does_not_finalize` — no manifest → nothing registered, note set (failure does not finalize — pinned). The placeholder-fields finding from the elaboration: `skill_name` is empty for manifest-registered adapters (the manifest carries no skill identity), so the A/B comparison only runs for pre-registered retrain adapters — that is the existing designed boundary (the pre-registration path carries the skill name), not a defect; the manifest path's metrics (loss/duration) ARE durable (pinned).

**Verification (2026-09-09):** hkask-mcp-training **18 passed** (15 + 3 new), 0 failed; clippy exit 0; rustfmt clean.

### T14 — IPC discovery convention — VERIFIED 2026-09-09 (Group 3)

**Defect (confirmed by inspection — the executable RED requires controlling the process UID, which is not portable):** the PUBLICATION side hardcoded `/run/user/1000` as the XDG fallback while the DISCOVERY side already resolved the real UID from `/proc/self/status` (its own comment: "Use the actual UID from the environment, not a hardcoded 1000") — the two sides diverged, and discovery silently failed for every non-1000 user.

**Fix:** ONE shared resolver — `hkask_inference::inference_ipc_client::runtime_dir()` (+ the pure `runtime_dir_with(xdg, uid)` core) — used by BOTH publication (`kask_bridge::inference_socket`) and discovery. Resolution: non-empty `XDG_RUNTIME_DIR` wins → `/run/user/{uid}` from `/proc/self/status` (std-only, no unsafe/libc) → `None` when neither resolves (callers treat it as "no location" — publication warns and skips; discovery returns None; NEVER a hardcoded UID, which would write/read in another user's directory). The discovery's old silent uid=1000 proc-failure fallback is deleted with the unification.

**Evidence:** `runtime_dir_resolution_table` — XDG wins; empty XDG ignored; per-UID for uid 1001 and 0; `None` when unresolvable; the live resolver agrees with the pure core for the current process. The publication wiring compiles against the shared resolver (kask_bridge suite green).

**Verification (2026-09-09):** hkask-inference **54 passed** (53 + 1 new), kask_bridge **187 passed**, 0 failed; clippy exit 0; rustfmt clean.

### T15 — Skill-feedback sensing — spec recovery COMPLETE; implementation gated on an operator routing decision

**Recovery findings (2026-09-09):** the skill OUTCOME writer IS wired — `crates/zed/src/main.rs:970-995` wires `agent::set_skill_outcome_recorder` → `record_skill_span(skill_id, "outcome", payload)` into the shared RegulationLedger (landed 2026-09-08; the comment documents that before it "the read side shipped with no writer — the store was permanently empty and drift sensing could never fire"). The drift consumer's outcome half is live. **The `operator_feedback` writer NEVER EXISTED:** the storage API (`record_skill_span`'s phase parameter, runtime.rs:39-54) and the reader (metacognition.rs:432-435 — the "declining operator acceptance" trend, adversarial review finding 3) both ship, but zero production code writes `operator_feedback` spans — that half of the drift sensing reads a permanently-empty phase and can never fire. The intended capability is real (commit `df6095f09b` added both phases' storage and readers together); the producer was never wired — "unwired" is not "unwanted".

**The routing decision returned to the operator (per the task's own gate — no invented policy):** WHAT production event constitutes operator feedback on a skill? Candidates: (a) the curator's advice-apply flow (`curator_advice_mark_applied` — the operator acting on a skill's advice is acceptance feedback), (b) a direct operator action (a panel control or tool call), (c) the gemba loop's operator review outcomes. The reader expects per-skill disposition payloads (acceptance rates). The operator picks the source; the wiring then follows the outcome-recorder precedent (a settable hook → `record_skill_span(skill_id, "operator_feedback", payload)`).

**Group 3 net-LOC:** status.rs +197/−87 (the extraction + typed error), hkask_mcp_training.rs +126 (the fixture + 3 tests), inference_ipc_client.rs +~55 (the shared resolver + table test), inference_socket.rs +~15/−10 (the unified publication), grounding.rs 2-line typos fixture fix ("sorced"→"obscure" — a gate-forced fix in a parallel-session file, recorded per the coordination constraint).

**Anchor:** `kask/mcp-servers/hkask-mcp-training/src/tools/status.rs:134`. **Scope: M. Depends: recover the completion-manifest test seam first.**
**Defects found (verified):** (1) `uuid::Uuid::parse_str(&job_id).unwrap_or_default()` — a malformed job id silently becomes the nil UUID (the .rules silent-fallback trap; the lookup then misses or hits an unrelated row). (2) The adapter built from the completion manifest carries placeholder fields (`String::new()` for two, `0`, `1`) — durable metrics/artifact information may be incomplete. (3) Idempotence of the repeated poll on the register path is unproven (the pre-registered path reports "Already registered"; the fresh-register path calls `adapter_store.store` every poll until the store dedups — verify). (4) Failure does not finalize — verify a failed manifest read leaves no adapter.
**Fix:** nil-UUID fallback → typed error; complete the manifest→adapter field mapping; prove poll idempotence; failure control.
**Acceptance:** real pre-registration + fixture-backed successful manifest updates durable metrics/artifact info and the A/B comparison; repeated poll is idempotent (no duplicate adapters, no metric churn); failure does not finalize.
**Verification:** recover the completion-manifest test seam (a fixture manifest the status tool can read) first; then RED-first per defect.
**Refused shortcut:** asserting only the happy path; leaving the nil-UUID fallback in place.

### Scheduling proposal (groups of 2–3 with cumulative checkpoints, per the plan)

- **Group 1 — evidence fidelity (memory/event substrate):** T09 + T10. Both S; both are "the recorded evidence must survive to the reader" defects in the memory/event layer. Cumulative: hkask-memory + kask_bridge suites.
- **Group 2 — measurement integrity (calibration/regulation):** T11 + T12. Both S/M; both are "evidence destroyed or deduped before observation" defects. Cumulative: prediction-markets + hkask-regulation suites. T11 carries the legacy-migration operator gate (additive-only default).
- **Group 3 — lifecycle/infra:** T13 + T14 + T15. T13 and T14 have confirmed defects (S/M); T15 is gated on spec recovery — schedule it last in the group so the recovery work (read-only) can start anytime without blocking T13/T14.

Each group ends with a checkpoint: cumulative regressions, build/lints, operator review before the next group.

## D01 — Shared-database retention decision

**Policy owner:** operator. **Decision:** YES, ratified 2026-09-07 by "D01 yes. please proceed". **Implementation/verification owner:** receiving coding agent. Policy is resolved and the implementation is verified (see Validation below — the header's earlier "not yet verified" note was stale and is corrected 2026-09-09).

Companies research notes and forecasts must survive portfolio schema recovery. The decision and superseded whole-file-disposal scope are recorded in `kask/docs/reference/mcp-servers/portfolio.md` and `companies.md`. Preserve attachment metadata and referenced portfolio parents too. Portfolio-only legacy data remains disposable; no research migration is authorized by this decision.

**Implemented approach:** `open_with_schema_recovery` in `kask/mcp-servers/hkask-mcp-portfolio/src/store.rs` uses an IMMEDIATE transaction for DDL, ownership inspection, and recovery. Any non-internal table outside the five portfolio tables prevents a destructive reset. Portfolio-only recovery drops owned tables child-first and rebuilds them transactionally; failures roll back. No database unlink/reopen remains. Incompatible shared databases return an explicit error; compatible shared databases open normally. This availability trade-off was communicated to the operator; do not silently add migration or cascade deletion to avoid the error.

**Five tests in `src/tests.rs`:** `schema_recovery_discards_stale_portfolio_data` (existing control adapted), `schema_recovery_preserves_mixed_database`, `schema_recovery_refuses_unknown_non_portfolio_table`, `schema_recovery_rolls_back_failed_portfolio_reset`, `schema_recovery_opens_compatible_mixed_database`. Mixed fixtures preserve notes/files/forecasts, revisions/outcomes, parent rows, schema, and FK integrity. The helper is `pub(super)` for the crate-local seam; no new public API/dependency was added.

**Validation (executed 2026-09-07):** pre-fix RED observed via a throwaway probe (the HEAD unlink/recreate recovery destroyed seeded research rows in a mixed DB; probe deleted after capture). Post-fix: `cargo test --offline --locked -p hkask-mcp-portfolio --lib schema_recovery` → 5/5 passed; full crate suite → 44 lib + 2 + 5 + 2 across targets, 0 failed; companies compatibility → `--lib` 68/68 passed; `cargo check` and `./script/clippy -p hkask-mcp-scenarios -p hkask-mcp-portfolio` clean (clippy release/all-targets/all-features `--deny warnings`, machete/typos/buf clean). Enforcement citations updated in `portfolio.md` and `companies.md`. The user-visible trade-off (incompatible shared DB errors at startup) remains as communicated; no research migration was added.

## Risks and open gates

Probabilities/frequencies are unmeasured. Severity is conditional on reaching the audited path, not a claim of incident frequency.

| Risk if outstanding | Severity | Owner / closure path | State (2026-09-09) |
|---|---|---|---|
| Private-network extraction through redirects | High | Coding agent, T02 regression and transport gate | **CLOSED** — T02+T02b verified 2026-09-07 |
| Lost recovery journal or undistilled turns | High | Coding agent, T01/T03; T04 prevents skipped learning | **CLOSED** — T01/T03 verified 2026-09-07; T04 verified 2026-09-08 (128-bound ratified at Checkpoint B) |
| Over-authorized spending or duplicate spawned work | High | Coding agent, T05/T06; operator ratifies any unresolved settlement policy | **CLOSED** — T05 (hold-on-ambiguity, ratified) + T06 verified 2026-09-08; Checkpoint B ratified |
| False compliance or lost explicit escalations | High | Coding agent, T07/T08; no false durable-success claims | **CLOSED** — T07/T08 verified 2026-09-09; Checkpoint C ratified |
| Cross-domain research loss during schema recovery | High | D01 policy ratified; receiving agent must validate the implemented guard before closure | **CLOSED** — D01 implemented + validated 2026-09-07 (stale header corrected 2026-09-09) |
| Stale vectors, biased calibration, misleading harness/cap feedback | Medium–High | Coding agent, elaborate T09–T12 at Checkpoint C | **CLOSED** — T09–T12 verified 2026-09-09 (Groups 1–2) |
| Incomplete adapter metadata, unavailable fallback IPC, unsensed skill drift | Medium | Coding agent, elaborate T13–T15; recover interfaces/spec first | **T13/T14 CLOSED** (verified 2026-09-09, Group 3); **T15 OPEN** — the operator_feedback producer awaits the operator's routing decision |
| Missing production-shaped test seams increase task size | Medium | Coding agent, mandatory pre-task sizing/design gate | Standing discipline — the T13 manifest seam was recovered before implementation (the precedent held) |
| Dependency compilation exceeds the current 180-second window | Verification blocker | Receiving agent coordinates a longer bounded window; preserve incremental cache; no repeated timeouts, lock deletion, or killing other builds | Standing constraint — 30-minute windows in use; the external release build was killed once by operator order (2026-09-09) |

Unresolved details are gates, not permission to invent behavior. **Resolved gates** (with their resolutions): mixed-age deletion policy (T03 — hard deletion only for fully-covered entities), bounded pending-work behavior (T04 — 128 threads, evict-oldest + warn, ratified), unknown-outcome credit reconciliation (T05 — hold on ambiguity, release only on proven pre-dispatch rejection, ratified), acknowledgment/delivery semantics (T07/T08 — truthful outcomes; queue-only escalation routing, ratified), legacy calibration evidence (T11 — additive-only, identity-less rows never dedup, no migration needed). **The one remaining gate: skill-feedback ownership (T15)** — the operator_feedback producer's source event (curator advice-apply / direct operator control / gemba review outcomes).

## Definition of done and verification

For each implemented slice:

1. Observe the new regression fail on the pre-fix path for the intended reason. Compilation failure is not RED evidence of the behavioral defect.
2. Observe the regression and controls pass after the fix. Use capability-complete constructors; no empty-result degradation-as-success assertions.
3. Run targeted tests first, then affected crate test suites and `cargo check`, followed by `./script/clippy -p` for affected packages. Use bounded timeouts and limited parallelism. GPUI tests use its tracked executor; live-mutation suites, if separately authorized, run single-threaded.
4. Run `bash kask/scripts/check-mcp-tool-tests.sh` as a presence ratchet, never as proof of transition coverage. At checkpoints build the affected application integration where touched; any blocked build is reported as blocked, not waived.
5. Review diff scope, stale comments, obsolete dependencies, temporary fixtures, changed error/schema behavior, and documentation against recovered intent. No commits/branches without explicit instruction.
6. Record exact commands, observed output, test names, and residual risks. A task is not complete when verification is blocked.

Planning baseline: earlier audit's MCP test-presence ratchet passed. Its test build timed out waiting for a lock; implementation-phase builds instead progressed through dependency compilation. Exact attempted commands and outcomes are in the continuation prompt. Neither crate had been compiled to completion or tested at handoff; that blocker is now resolved — see the evidence record below.

### Verification evidence — 2026-09-07 (T01 + D01, all commands from the repository root)

| Command | Observed result |
|---|---|
| `cargo test --offline --locked -p hkask-mcp-scenarios --test tool_behavior -j 2` | 18 passed, 0 failed (persistence regressions included) |
| `cargo test --offline --locked -p hkask-mcp-portfolio --lib schema_recovery -j 2` | 5 passed, 0 failed |
| Pre-fix RED (worktree-only `git show HEAD:… > …` reverts; index untouched): scenarios `--test tool_behavior` | `snapshot_failure_preserves_journal_for_recovery`, `journal_failure_is_surfaced_before_memory_changes`, `snapshot_with_uncleared_journal_recovers_exactly_once` each FAILED for the intended reason ("persistence/journal failure must reach the caller" / score succeeded where a persistence failure should surface); 15 passed, 3 failed |
| Pre-fix RED, portfolio: throwaway `tests/d01_red_evidence.rs` probe (deleted after capture) | FAILED: research row read back as `"DESTROYED"` vs `"Durable research"` — HEAD's unlink/recreate recovery destroyed it |
| Post-restore GREEN re-run: scenarios `--test tool_behavior` | 18 passed, 0 failed |
| `cargo test --offline --locked -p hkask-mcp-scenarios -p hkask-mcp-portfolio -j 2` | 44 + 2 + 5 + 2 + 18 passed across targets, 0 failed |
| `cargo test --offline --locked -p hkask-mcp-companies --lib -j 2` | 68 passed, 0 failed (shared-DB compatibility) |
| `cargo check --offline --locked -p hkask-mcp-scenarios -p hkask-mcp-portfolio -p hkask-mcp-companies -j 2` | Finished, no errors |
| `./script/clippy -p hkask-mcp-scenarios -p hkask-mcp-portfolio` | clean: release/all-targets/all-features `--deny warnings`, plus machete (scoped to `kask/`), typos, buf lint/format; 35m49s wall |
| `bash kask/scripts/check-mcp-tool-tests.sh` | 0 violations, 0 allowlisted gaps (presence ratchet only) |
| `git diff --check` / `git diff --cached --check` | clean |
| `rustfmt --edition 2021 --config skip_children=true --check` on touched scenarios test file | clean |

### Verification evidence — 2026-09-07 (T02 core slice, all commands from the repository root)

| Command | Observed result |
|---|---|
| `cargo test --offline --locked -p hkask-mcp-research -j 2` | lib 34 passed (6 inline redirect/resolver regressions: loopback-literal sentinel zero, 169.254.169.254 rejection, localhost-name resolver rejection with zero sentinel requests, browse variant, decision bound/cycle/forbidden/permitted, resolver unit), tool_behavior 30 passed (4 outer-gate tests), 0 failed |
| `cargo test --offline --locked -p hkask-mcp-server --lib -j 2` | 22 passed including the 3 new validator tests (unspecified, NAT64/compatible, resolved-address list), 0 failed |
| Pre-fix RED (worktree-only `git show HEAD:… > …` reverts; index untouched): throwaway transport probe | FAILED as intended — pre-fix RawFetch followed the redirect to the loopback sentinel and returned its content `Ok("sentinel-body")`; probe deleted after capture |
| Pre-fix RED: `--test tool_behavior` (kept new, reverted production) | `web_extract_rejects_unspecified_destination`, `web_extract_rejects_nat64_loopback_destination`, `web_browse_rejects_unspecified_destination` FAILED (validator admitted them; pool returned PermissionDenied, not InvalidArgument); loopback control passed |
| `cargo check --offline --locked -p hkask-mcp-server -p hkask-mcp-research -j 2` | Finished, no errors |
| `cargo test --offline --locked -p hkask-mcp-media -j 2` (consumer compatibility: the media tools validate remote media URLs through the same shared strict validator that T02 tightened) | 210 + 61 + 1 passed, 0 failed |
| `./script/clippy -p hkask-mcp-server -p hkask-mcp-research` | clean: release/all-targets/all-features `--deny warnings`, machete/typos/buf clean (3 test-code lints fixed: slice-from-ref, redundant clone) |
| `rustfmt --check` on the three touched Rust files | clean |

### Verification evidence — 2026-09-07 (T03 forgetting coverage + operator-directed reliability addenda)

| Command | Observed result |
|---|---|
| Test-first RED: the three new forgetting regressions against the pre-fix pass | `forgetting_preserves_turns_newer_than_the_watermark` FAILED (2 turns deleted, expected 1 — the uncovered turn was swept by the prefix delete), `forgetting_keeps_ambiguous_passage_embeddings` FAILED (3 deleted, expected 2), `forgetting_skips_threads_without_a_parseable_watermark` FAILED (thread forgotten without coverage proof) |
| `cargo test --offline --locked -p hkask-mcp-curator --lib -j 2` | 16 passed (forgetting 7/7: 3 new + 4 existing pins on corrected fixtures), 0 failed |
| `cargo test --offline --locked -p hkask-memory -j 2` / `-p hkask-storage --lib -j 2` | 30 passed; 60 passed — twice consecutively (182.6s, 179.8s) |
| `./script/clippy -p hkask-storage -p hkask-memory -p hkask-mcp-curator` | clean (release/all-targets/all-features `--deny warnings`, machete/typos/buf) |

**Storage-suite parallel flake — root-caused and fixed (operator directive "fix the pre-existing parallel load flake").** Three distinct mechanisms, each A/B-verified against HEAD:

1. **KDF saturation → r2d2 30s timeout** (all `SqlCipher("timed out waiting for connection")` failures): r2d2's `min_idle` defaults to `max_size`, so every SQLCipher pool build eagerly established 8 connections × ~2s PBKDF2 each (measured: one KDF test = 40s = ~18 KDF rounds at HEAD). The suite's aggregate KDF demand saturated the CPU and any `pool.get()` landing in the window tripped r2d2's 30s default. Fixed: `min_idle(Some(0))` — strictly on-demand establishment (the pool's lifetime is exactly its handles'), plus `connection_timeout(120s)` (a local KDF-bound connection is worth waiting for; failing a recoverable get() was the spurious broken loop). Result: the same tests run ~2.6× faster (40s→15s, 39.9s→15s) and server startup drops the eager 8-KDF tax. An intermediate `min_idle(Some(1))` attempt was rejected with evidence: r2d2 replenishes to `min_idle` **on every checkout** (`try_get_inner`), and an in-flight replenish task keeps the manager — and its maintenance lease — alive past the last handle drop, which deterministically blocked exclusive lease acquisition (`rotation_exclusive_lease_blocks_new_pool_admission` failed alone at 5.2s, passed at HEAD; strace showed the shared flock never released).
2. **Catalogue read-modify-write race** (`catalogue_serializes_writers_without_losing_external_paths`, 7≠8): `record_catalog_path` parses → dedupes → seeks to end → appends with **no lock** (the reader takes `lock_shared`; the writer never took the exclusive side). Two writers seek to the same end offset and one overwrite loses a record. Measured **6/10 failures** pre-fix → **10/10 green** post-fix (`file.lock()` covering parse→append). This writer runs on production server-startup paths — a real data-loss race, not just a test flake.
3. Full suite: **60/60 twice consecutively** at default harness parallelism (previously failing every run).

**Dead/redundant-code sweep (operator directive), scoped to session-touched crates.** Removed: `MemoryStore::with_storage_budget` + `default_storage_budget` (zero callers, self-documented never-wired, and count-based budgets are deprecated by operator ruling 2026-09-04; the live `storage_budget()` accessor stays — it feeds the regulation storage-ratio set-point via `kask_bridge`); 6 pre-existing `cloned_ref_to_slice_refs` clippy failures in `maintenance_inventory.rs` (`std::slice::from_ref`, which had been failing `./script/clippy -p hkask-storage` before this session). **Flags raised, then resolved by operator ruling 2026-09-07 (Phase D):**

- `kask.memory.memory_life_days` — the setting feeds the regulation sensor while the store ignores it (decay always 180) → **T16, build the wiring.**
- `consolidation_service.rs` — this flag was **RETRACTED as false**: the initial "zero external references" claim was a grep artifact (output truncated before the kask_bridge hits). Consolidation is fully wired: `zed/src/main.rs:1764` starts the production timer → `fire_curator_consolidation_pass` (`memory.rs:283`) → `MemoryConsolidator::consolidate` (`memory.rs:439`) with the settings' `confidence_floor`; rebuilt after heals (`ingest.rs:120-128`); callback tested (`memory.rs:1978`). That was the initial retraction, not evidence of firing. Subsequent production-timer RED found the never-fires defect; **T17 now includes the scheduling repair**, as recorded in the 2026-09-08 evidence.
- `salience.rs` method-signals half — orphaned by the condenser removal while the module doc claims consumers that don't exist → **T18, build the wiring.**

### Verification evidence — 2026-09-07 (T02b discover-path slice)

| Command | Observed result |
|---|---|
| Test-first RED (before the fix, current tree): throwaway probe `feed.rs` `t02b_red_probe` driving `discover_feeds` with the handler's client class (plain default) against a loopback redirect fixture | FAILED as intended — the redirect was followed and the sentinel fetched (`discover returned: Ok([])`); probe deleted and replaced by the permanent tests |
| `cargo test --offline --locked -p hkask-mcp-research -j 2` (post-fix) | lib 36 passed (new: `discover_feeds_rejects_redirect_to_loopback` with named reason + zero sentinel requests, `discover_feeds_direct_fetch_still_works` control), tool_behavior 31 passed (new: `rss_discover_feeds_rejects_unspecified_destination`), 0 failed |
| `cargo check --offline --locked -p hkask-mcp-research -j 2` | Finished, no errors |
| `./script/clippy -p hkask-mcp-research` | clean (release/all-targets/all-features `--deny warnings`, machete/typos/buf) |
| `rustfmt --edition 2024 --check` on the six touched research-crate files | clean |

T02 scope/residue: files touched are the four declared ones (`security.rs`, `server.rs` re-export line, `raw_fetch.rs`, `tests/tool_behavior.rs`) plus `research.md` and these task docs; the throwaway probe and its `mod` wiring were removed (the out-of-band staging process had captured the probe mid-RED; it was verified absent from index and worktree afterward); stale-comment sweep done — the security.rs "future hardening" TOCTOU comment was rewritten in the same change to state that the raw-fetch transport closes the gap at connect time and other consumers keep the documented gap. T02b (`rss_discover_feeds` shared-client redirect hole) is recorded as an open follow-up slice.

Scope/residue review 2026-09-07: no files changed outside the declared boundary (two scenarios production files + their test file + Cargo.toml/Cargo.lock edge, portfolio store + tests, three reference docs, plan/todo); the throwaway RED probe was deleted; `/tmp/zed-kask-red-backup/` scratch backups are outside the tree; no stale comments found — the old warn-and-truncate/warn-and-admit comment blocks were updated with the implementation; machete confirms the new `tempfile` dependency is used. Operator review at Checkpoint A remains open.

## Refinement history

Independent proposal review scored deficiency 0.1625 (gate 0.15): failure boundaries and sizing needed clarification. Revised T05/T06 to cover external acceptance, persistence failure, concurrent retries, and restart; constrained T04 retry to preserve watermark/startup contracts; separated scheduling from technical dependencies; named T08 sinks and their best-effort limitation; explicitly marked follow-up headings not execution-ready. Added the unresolved policy gates and cumulative checkpoint requirements. Final plan review is recorded separately in the delivery message; no implementation correctness is implied by plan quality.

## Process anchors

- `pko:Procedure`: this regression-first program; `pko:ProcedureTarget`: the target condition above.
- T01–T15: `pko:Step`; each acceptance/verification field is its `pko:StepVerification`. T09–T15 require elaboration before execution.
- Phases A–C: `pko:MultiStep`; Checkpoints A–C: `pko:UserFeedbackOccurrence`.
- Risk rows: `pko:IssueOccurrence`; D01 and unresolved policy gates: `pko:UserQuestionOccurrence`.
- This document carries Dublin Core-style title/creator/date and `bibo:Document` metadata; [todo.md](todo.md) is its execution checklist.
