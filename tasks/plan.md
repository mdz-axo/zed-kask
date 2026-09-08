---
title: "Kask regression-first reliability plan"
creator: "Zed coding agent"
date: "2026-09-07"
type: "bibo:Document"
status: "Phase D automated checks and release builds verified; live quit and operator checkpoints pending"
baseline: "2475305420ae065b5d1792c0f25cea471e558ae3"
---

# Kask regression-first reliability plan

## Target condition

Authorization history: the operator first requested this regression-first plan, then on 2026-09-07 said **"D01 yes. please proceed"**, ratifying research retention and authorizing implementation. The latest instruction resumes the Phase D plan after ratifying the development-profile Clippy wrapper. The T16/T17/T19 validation gate was closed before implementing T18; Phase D implementation and automated checks are now complete. No commits, branches, unrelated policy changes, or acceptance of deferred risks were authorized.

The target is to prevent the identified loss of recoverable data, private-network boundary bypass, over-authorized external dispatch, duplicate agent creation, and false completion evidence. Fix one observable failure path at a time; deepen existing modules only after behavioral tests pass.

Audit findings are source-backed hypotheses with concrete triggers, not reproduced incidents. **T01–T03 (including T02b) and D01 are verified; T04–T15 remain untouched. T16–T19 have passing automated tests/checks/lints; operator review and a live application-quit smoke test remain at Checkpoint D.** Owner for technical closure is the receiving coding agent; unresolved policy decisions remain with the operator. Later-wave tasks remain tracked, not silently abandoned or declared safe.

## Current validation close-out — 2026-09-08

**Current continuation:** [Final verification and remaining program continuation](kask-reliability-final-verification-continuation.md). Its unstarted-build statement is superseded by the release-build evidence below. Both build-only commands passed; normal editor quit and operator reviews at Checkpoints A/D remain unobserved. The reliability program is **not complete**.

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
| Live shutdown | **Not observed.** No watcher has been armed and no process was stopped. Refresh PID/start-time identity immediately before the operator-controlled test. |

**Durable logs:** `target/kask-reliability-rebuild-20260908T222941Z/{editor,servers}.log`; companion `{editor,servers}.json` record exact commands, revision, timestamps, wrapper, exit status and elapsed time. `live-executable-identities.json` records the 12 PID/parent/start-time identities, executable paths, SHA-256 values and ELF build IDs. Existing earlier test evidence below was not indiscriminately rerun; no new behavioral RED/GREEN is claimed for this evidence-only slice.

**Next operator-controlled steps:** confirm Checkpoint A review; arrange an independent bounded read-only watcher, refresh owned child identities, then let the operator normally quit the build-ID-matched editor. Record child termination/reaping and any survivors without blanket-killing services. Confirm normal quit with the operator (process disappearance alone does not establish how it exited). The isolated pending-start/nonresponsive fixture evidence remains in the T19 table; it does not replace a live application result. Source inspection confirms `--user-data-dir` exists, but no isolated app was launched or complete Kask-data/keychain isolation claimed.

**Closure ledger:** release-build verification — fixed/verified, coding agent; executable identity — verified by ELF build IDs, coding agent; Checkpoint D live quit/review — awaiting operator coordination, coding agent observes and operator confirms; Checkpoint A review — operator decision, blocks Phase B; T04–T08 — coding agent queue after review and task-specific policy gates; T09–T15 — coding agent elaboration plus operator scheduling, not execution-authorized. Deferral risks remain in the risk register; no acceptance of those risks is inferred.

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

Acceptance:
- A turn skipped as active becomes eligible and is distilled on a later production-shaped pass without requiring a new turn or restart.
- A transient inference failure before watermark advancement is retried; successful work is not replayed unnecessarily.
- Ratified watermark-before-lesson ordering and bounded startup discovery remain intact. Pending-work bounds/overflow must be explicit and non-silent; if the contracts conflict, stop for a decision rather than silently broadening retention or replay.

**Verification:** successive passes driven through the production orchestration/cursor, scripted inference, and controlled time; include success→repeat and failure→success. Preserve T03 integration coverage. Do not test only the helper with an epoch-wide scan.
**Refused shortcut:** moving the cursor without tracking unresolved work, or silently rescanning all historical memory.
**Skill match query:** asynchronous discovery progress versus completion-state test design.

### T05 — Reserve session authorization before external dispatch

**Scope:** M, re-slice if necessary; trust; `hkask-mcp-swarm`. **Depends on:** none. **Policy gate:** unknown-outcome reconciliation/refund behavior must be recovered or ratified before implementation.
**Likely files:** `kask/mcp-servers/hkask-mcp-swarm/src/spend_gate.rs`, `src/consent.rs`, existing server/consent tests.
**Anchor:** `authorize_delegate` → external POST → `complete_delegate` → durable consent state.

Acceptance:
- Two overlapping 10-credit authorizations against one 10-credit session admit at most one POST, including separate server instances sharing the consent DB.
- Success settles once; a proven pre-dispatch rejection releases only its own reservation; per-dispatch ceilings and single-use consent behavior remain intact.
- External acceptance followed by transport/local-persistence failure retains durable reservation evidence across reopen and reports uncertainty. A retry cannot reuse the same reserved capacity; no unsupported promise of external exactly-once delivery is made.

**Verification:** real SQLite consent store, barrier-controlled bounded HTTP fixture, failure before send/after acceptance/before local finalization, and reopen tests. Pin a reviewed reconciliation policy, not an invented timeout refund.
**Refused shortcut:** atomic deduction after the POST or automatically refunding every transport error.
**Skill match query:** atomic consent reservation, ambiguous external outcomes, concurrency tests.

### T06 — Preserve replay protection after agent creation

**Scope:** M; composition/lifecycle; `hkask-mcp-kata-kanban`. **Depends on:** none.
**Likely files:** `kask/mcp-servers/hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs`, `src/idempotency.rs`, `tests/idempotent_creates.rs`.
**Anchor:** `with_idempotency` → successful spawn → fallible task comment → same-key retry.

Acceptance:
- Post-spawn bookkeeping failure followed by same-key retry creates exactly one agent; result is replayable or explicitly pending/partial.
- Concurrent same-key requests and retries after reopening durable state preserve the claim within the existing TTL, including failure after external acceptance before response recording.
- Proven clean pre-effect validation failures release the claim and can be retried successfully; ephemeral-goal semantics remain unchanged.

**Verification:** counting successful spawn port, real persistent replay store, one-shot post-spawn database fault, simultaneous retry and reopen controls. Exercise both worktree/local paths where their error classification differs.
**Refused shortcut:** classify all errors as clean, or test only an unavailable spawn port.
**Skill match query:** idempotent effectful tool retries and partial-success failure injection.

**Checkpoint B:** cumulative memory, authorization, and spawn regression checks; scoped build/lint evidence; explicit resolution of T05's policy gate; operator review. Serial editing is a resource constraint, not a T04→T05→T06 technical dependency.

### Phase C — Make regulation acknowledgments truthful

### T07 — Report actual directive outcomes

**Scope:** M; curation; `hkask-regulation`. **Depends on:** none.
**Likely files:** `kask/crates/hkask-regulation/src/cybernetics_loop/directive.rs`, existing regulation tests; acknowledgment consumers only if the recovered schema requires it.
**Anchor:** directive inbox → application handler → acknowledgment/event sink.

Acceptance:
- Applied acknowledgment requires evidence of the actual supported effect; failures and unsupported/log-only variants are explicitly distinguished.
- Dampened input is not represented as applied; working cap/threshold operations preserve their behavior.
- No capability authority or autonomous actuator is added to satisfy a formerly misleading acknowledgment; incompatible acknowledgment schema changes require review.

**Verification:** table of existing directive variants through inbox processing with real state assertions and a recording event sink; handler failures must not become success.
**Refused shortcut:** rename a log message while continuing to persist false completion.
**Skill match query:** typed command outcomes and production dispatch contract tests.

### T08 — Deliver explicit domain escalations

**Scope:** M; curation; regulation/bridge seam. **Depends on:** T07.
**Likely files:** `kask/crates/hkask-regulation/src/cybernetics_loop/directive.rs`, `src/cybernetics_loop/cycle.rs`, `src/algedonic.rs`, `kask/crates/kask_bridge/src/directive_bridge.rs`, existing bridge alert tests.
**Anchor:** `EscalateDomain` → inbox → existing `AlertEscalationSink` / `BridgeAlertEscalationSink` durable queue and existing alert channel.

Acceptance:
- An undampened explicit escalation retains domain, severity, and evidence in the human-review path; test queue persistence and alert delivery independently.
- Missing/broken sinks are surfaced; queued, attempted, and confirmed durable delivery are not conflated. The existing void/best-effort sink does not prove persistence success—recover or narrowly adapt its contract before claiming durable acknowledgment.
- Existing dampening/general alerts remain functional, without treating a requested escalation as a fabricated measured threshold breach.

**Verification:** bridge→inbox→real temporary escalation store plus channel receiver, missing-sink control and write-failure variant. If the existing sinks cannot faithfully carry an explicit concern, return the routing decision to the operator rather than invent policy.
**Refused shortcut:** record only the directive type as applied or invent autonomous remediation.
**Skill match query:** human escalation delivery with truthful persistence and channel outcomes.

**Checkpoint C:** cumulative directive and memory/budget regressions, build/lint evidence, and operator review before scheduling the follow-up queue.

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

## Follow-up queue: tracked, not execution-ready

These are not accepted deferrals or implementation authorization. Each remains owned by the coding agent for elaboration at Checkpoint C (or earlier if the operator reprioritizes). Before scheduling, expand each into the same three-criterion/failure-control format as T01–T08; re-slice anything larger than M. Source anchors and minimum completion evidence follow.

| ID | Target / source anchor | Required evidence before completion | Dependency / gate | Skill match query |
|---|---|---|---|---|
| T09 | Passage deletion ownership — `kask/crates/hkask-memory/src/memory_store.rs:596` | Delete/prune one passage; its stored text/embedding disappears, sibling chunks survive, and the next valid semantic match remains retrievable. | Reconcile with T03's actual deletion identity; no mandatory architectural dependency on T03. | Memory identity and relational/vector lifecycle consistency |
| T10 | Exact harness comparison — `kask/crates/kask_bridge/src/rollout_event_bridge.rs:132` | Production detector→verification preserves the exact 0.9→0.6 pair in a 0.4→0.9→0.6 history; interleaved verdicts do not suppress evidence; metric identity is retained. | None; recover both summary identities, not merely reverse an iterator. | Event history selection and regression feedback fidelity |
| T11 | Market-identity calibration — `kask/mcp-servers/hkask-mcp-prediction-markets/src/calibration.rs:103` | Five distinct 0.9/no markets yield five samples and Brier 0.81; rescans add zero; old stored observations are handled without fabricated identities. | Operator gate if legacy-data migration changes retained evidence; re-slice migration separately. | Calibration identity, deduplication and compatible persistence |
| T12 | Cap-reset evidence — `kask/crates/hkask-regulation/src/cybernetics_loop/cycle.rs:420` | Exhaustion is observed before replenishment; reset alone earns no advice-progress credit; dispatch limits still hold. | Reconcile T07/T08 edits to shared regulation code; no semantic dependency assumed. | Feedback measurement isolation across resource replenishment |
| T13 | Retrain finalization — `kask/mcp-servers/hkask-mcp-training/src/tools/status.rs:134` | Real pre-registration followed by fixture-backed successful manifest updates durable metrics/artifact information and comparison; repeated poll is idempotent; failure does not finalize. | Recover completion-manifest test seam first. | Adapter lifecycle finalization and external completion fixtures |
| T14 | IPC discovery convention — `kask/crates/kask_bridge/src/inference_socket.rs:49` | Publication→discovery works for absent/empty/populated XDG and non-1000 UID; env precedence/private directory protections remain intact. | No new upstream edits without a D-seam decision. | Secure runtime path publication and discovery round trips |
| T15 | Skill-feedback sensing — `kask/crates/hkask-regulation/src/runtime.rs:776` | Actual skill completion/operator feedback reaches the shared ledger and drift consumer with provenance; absent input remains explicitly unobserved. | Spec/writer ownership recovery before implementation; no deleting intended capability, invented skill identity, or substitute LLM success labels. | Spec-preserving feedback producer integration |

After elaboration, schedule follow-up checkpoints in groups of two or three tasks, each with cumulative tests/builds and human review. Do not treat this table as sufficient implementation design.

## D01 — Shared-database retention decision

**Policy owner:** operator. **Decision:** YES, ratified 2026-09-07 by "D01 yes. please proceed". **Implementation/verification owner:** receiving coding agent. Policy is resolved; the code is present but not yet verified.

Companies research notes and forecasts must survive portfolio schema recovery. The decision and superseded whole-file-disposal scope are recorded in `kask/docs/reference/mcp-servers/portfolio.md` and `companies.md`. Preserve attachment metadata and referenced portfolio parents too. Portfolio-only legacy data remains disposable; no research migration is authorized by this decision.

**Implemented approach:** `open_with_schema_recovery` in `kask/mcp-servers/hkask-mcp-portfolio/src/store.rs` uses an IMMEDIATE transaction for DDL, ownership inspection, and recovery. Any non-internal table outside the five portfolio tables prevents a destructive reset. Portfolio-only recovery drops owned tables child-first and rebuilds them transactionally; failures roll back. No database unlink/reopen remains. Incompatible shared databases return an explicit error; compatible shared databases open normally. This availability trade-off was communicated to the operator; do not silently add migration or cascade deletion to avoid the error.

**Five tests in `src/tests.rs`:** `schema_recovery_discards_stale_portfolio_data` (existing control adapted), `schema_recovery_preserves_mixed_database`, `schema_recovery_refuses_unknown_non_portfolio_table`, `schema_recovery_rolls_back_failed_portfolio_reset`, `schema_recovery_opens_compatible_mixed_database`. Mixed fixtures preserve notes/files/forecasts, revisions/outcomes, parent rows, schema, and FK integrity. The helper is `pub(super)` for the crate-local seam; no new public API/dependency was added.

**Validation (executed 2026-09-07):** pre-fix RED observed via a throwaway probe (the HEAD unlink/recreate recovery destroyed seeded research rows in a mixed DB; probe deleted after capture). Post-fix: `cargo test --offline --locked -p hkask-mcp-portfolio --lib schema_recovery` → 5/5 passed; full crate suite → 44 lib + 2 + 5 + 2 across targets, 0 failed; companies compatibility → `--lib` 68/68 passed; `cargo check` and `./script/clippy -p hkask-mcp-scenarios -p hkask-mcp-portfolio` clean (clippy release/all-targets/all-features `--deny warnings`, machete/typos/buf clean). Enforcement citations updated in `portfolio.md` and `companies.md`. The user-visible trade-off (incompatible shared DB errors at startup) remains as communicated; no research migration was added.

## Risks and open gates

Probabilities/frequencies are unmeasured. Severity is conditional on reaching the audited path, not a claim of incident frequency.

| Risk if outstanding | Severity | Owner / closure path |
|---|---|---|
| Private-network extraction through redirects | High | Coding agent, T02 regression and transport gate |
| Lost recovery journal or undistilled turns | High | Coding agent, T01/T03; T04 prevents skipped learning |
| Over-authorized spending or duplicate spawned work | High | Coding agent, T05/T06; operator ratifies any unresolved settlement policy |
| False compliance or lost explicit escalations | High | Coding agent, T07/T08; no false durable-success claims |
| Cross-domain research loss during schema recovery | High | D01 policy ratified; receiving agent must validate the implemented guard before closure |
| Stale vectors, biased calibration, misleading harness/cap feedback | Medium–High | Coding agent, elaborate T09–T12 at Checkpoint C |
| Incomplete adapter metadata, unavailable fallback IPC, unsensed skill drift | Medium | Coding agent, elaborate T13–T15; recover interfaces/spec first |
| Missing production-shaped test seams increase task size | Medium | Coding agent, mandatory pre-task sizing/design gate |
| Dependency compilation exceeds the current 180-second window | Verification blocker | Receiving agent coordinates a longer bounded window; preserve incremental cache; no repeated timeouts, lock deletion, or killing other builds |

Unresolved details are gates, not permission to invent behavior: mixed-age deletion policy (T03), bounded pending-work behavior (T04), unknown-outcome credit reconciliation (T05), acknowledgment/delivery semantics (T07/T08), legacy calibration evidence (T11), skill-feedback ownership (T15). Research retention (D01) is resolved; only its implementation verification remains open.

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
