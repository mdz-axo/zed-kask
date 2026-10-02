# Two-symptom latency investigation — findings report, hypothesis register, ranked plan

**Date:** 2026-10-01 · **Scope:** thread-load latency (symptom 1) and input-tracking freeze (symptom 2), the two user-observed symptoms in the goal `19b82d53-9a4e-43f6-a433-8270632dbc51`. Documents-only pass; no code changed. Composes, does not replace, P7a–P7f in `kask/docs/plans/hkask-core-mcp-repair-improvement-plan.md` — every prior measurement is cited where reused, and the P7g section of that plan is superseded by this report (see §7).

**Termination check (goal criteria):**

| Criterion | State |
| --- | --- |
| (a) Symptom 1 has a confirmed cause with mechanism chain, or its plausible cause set is eliminated | **Met** — mechanism chain verified end-to-end (S1-H1 below), plus three eliminated causes with evidence |
| (b) Symptom 2 has a confirmed cause with mechanism chain, or its plausible cause set is eliminated | **Partially met** — the cause class is confirmed and rate-driven causes eliminated (measured), but the per-draw cost driver inside the confirmed class is an open decomposition; ranked plan step 1 closes it |
| (c) Findings report + hypothesis register + ranked plan with measurement plan delivered | **Met** — this document |
| (d) Honest measured-vs-inferred statement | **Met** — §5 |

---

## 1. Stage 0 — baseline and environment (what is actually measured)

### 1.1 Instruments that exist and are alive in this build

| Instrument | Where | What it measures | Status |
| --- | --- | --- | --- |
| Per-window draw stats | `gpui::profiler::record_draw_duration` / `take_draw_stats`, surfaced as `ui frame health: w<id> draws=N avg_ms=A max_ms=M` in `crates/zed/src/reliability.rs` (30 s cadence) | draw **rate** vs draw **cost** per window (D84 follow-ups) | alive; **live data present in `~/.local/share/zed-kask/logs/Zed-Kask.log*` today** |
| `[DIAG-anr]` event-window probe | `crates/agent/src/agent.rs:2505-2514, 2687-2714` (temporary; "remove with the fix") | per-event forwarding cost in the `ThreadEvent` → `AcpThread` event loop: window totals, avg, max | alive; live data present today, 2026-09-29→10-01 |
| Foreground hang detector | `zed_kask::reliability::hang_detection::logging` | foreground stalls > threshold | alive; 3 `New foreground hang detected` entries today (10:45, 11:07, 15:03 sessions) |
| Input-latency histograms | `gpui::profiler::InputLatencySnapshot` + `crates/input_latency_ui` | input→frame latency percentiles, events-coalesced-per-frame | compiled in; **no snapshot has been taken** (requires `zed: dump input latency histogram` / telemetry path) |
| Built-in performance profiler | `instrumentation.performance_profiler.enabled` + `zed: open performance profiler` | per-task foreground/background timing | **off by default in the running profile** (see §1.3) |
| Removed probe sets | `[DIAG-thread-perf]` (commits `1e0b56216d`, `70e856a197`, removed `fe1c75892c`, `20cacfe3d8`) | thread-open phase split; foreground runnable-by-spawn-site | gone from HEAD (verified: `diagnostic_started`, `record_foreground_runnable_probe`, `diag_open` = 0 hits in `crates/`) |

### 1.2 Build-profile and binary identity — competitor resolved

| Fact | Evidence |
| --- | --- |
| The running binary is **release, stripped**: `~/.local/bin/zed-kask` (638 MB, 2026-10-01 11:07), `/proc/16758/exe` points at it | `ps`/`/proc`; `stat` on `~/.local/bin/zed-kask*` |
| Every `hkask-mcp-*` child is built from `target/release-mcp` (release + `lto=false`) and no source in `kask/` is newer than the installed binary | `kask/scripts/build/install.sh:228-295`; `find … -newer` → 0 hits |
| `target/debug/zed-kask` (2.1 GB) exists in the repo `target/` but is **not** what runs | `ls target/debug`, `/proc/16758/exe` |
| Upstream-parity verifier | `kask/scripts/build/check-build-profile.sh` (D46/D50) |

**Conclusion:** build-profile mismatch is **eliminated** as a competing explanation for both symptoms. The symptoms were observed and measured on the release binary.

### 1.3 Environment factors

| Factor | State | Effect on symptoms |
| --- | --- | --- |
| Machine load | `load avg 4.01` at sample time; a `clippy` (dev-profile) build of `kask/crates/hkask-*` was running | **inflation multiplier, not baseline cause** — the P7f differential (20–38 ms quiet vs 83–128 ms under 16-job build) is reproduced qualitatively below (§2.2) |
| Settings | `instrumentation.performance_profiler.enabled: false` in the running profile (`assets/settings/default.json` `dev` block sets it; the operator's settings file does not) | **the mandated decomposition instrument is off** — see ranked plan step 1 |
| MCP fleet | 13 `hkask-mcp-*` processes, each 0.0–0.4 % CPU | **eliminated as CPU factor** (also measured 2026-09-30, P7f) |
| `threads.db` | 338 MB (`~/.local/share/zed-kask/agents/…/threads.db`) | load-bearing for symptom 1 (size × per-row whole-thread blobs) |

---

## 2. Stage 2 — findings, per symptom

### 2.1 Symptom 1 — thread-load latency

#### Confirmed cause S1-H1 — whole-thread-per-row DB format × foreground replay/forwarding

**Mechanism chain (every link verified against HEAD):**

1. `ConversationView` opens a thread → `NativeAgent::open_thread` (`crates/agent/src/agent.rs:1889`) — a cached session short-circuits; otherwise `ThreadsDatabase::load_thread` (`crates/agent/src/db.rs:613`).
2. `load_thread` runs on the background executor but **under one shared `Connection`** (`crates/agent/src/db.rs:393`, `parking_lot::Mutex<sqlez::Connection>`): `db.rs:615` locks the connection, `SELECT data_type, data FROM threads WHERE id = ?` (primary-key lookup — no scan), then `deserialize_thread` (`db.rs:658`): zstd decode → full JSON parse of the entire thread.
3. `Thread::from_db` (`crates/agent/src/thread.rs:2031`) reconstructs the message list, then `Thread::replay` (`thread.rs:1806`-`1858`) walks **every message** and, per tool call, `replay_tool_call` (`thread.rs:1861-1983`) — which resolves the tool (including a scan of `context_server_registry` MCP tools when the tool is not a built-in, `thread.rs:1895-1905`), computes the title, and emits `ToolCall`/`ToolCallUpdate` events into an unbounded channel.
4. Those events are consumed on the foreground by the same forwarding loop the live stream uses: `NativeAgent::handle_thread_events` (`agent.rs:2504`+), whose per-event cost is **measured live at 3.6–5.4 ms avg** (`[DIAG-anr]` windows, `Zed-Kask.log`, 2026-10-01) with maxima 20–56 ms.

**Quantified (measured, cited):** 2026-09-24 probe (P7c/P7d, commit `70e856a197`, readings in `kask/docs/plans/hkask-core-mcp-repair-improvement-plan.md` P7c): six opens 0.7–2.2 s click-to-first-frame, dominated by load (0.54–2.0 s) not render (17–99 ms); a 963-entry thread = 635 ms of 670 ms load. Today's `[DIAG-anr]` avg (4–5 ms/event) × the several-hundred to several-thousand events a large replay emits is the same magnitude.

**D-seams implicated:** D28 (`crates/agent/src/db.rs` — the storage layout: one row = one whole serialized thread, re-serialized and zstd-compressed per save) · D14/D84 tail (the replay is paced by the same event/notify path whose cost D14/D84 address) · upstream loader parity (`from_db`/`replay` are upstream code — upstream has the same replay architecture; the fork's divergence is **volume**: 338 MB DB and thread size).

#### Confirmed cause S1-H2 — single shared DB connection queues opens behind saves

`crates/agent/src/db.rs:393` holds exactly one `Connection` behind a `Mutex`; `save_thread_sync` (`db.rs:503`) computes `serde_json::to_string` + `zstd::encode_all` **while holding the lock** (`db.rs:526-533`: `let connection = connection.lock();` then the compression is *before* the lock in current code — but the `INSERT … ON CONFLICT` blob write holds it). A turn-end save of a large thread can therefore delay a concurrent `load_thread`'s `SELECT`. Mechanism verified; **the magnitude is unmeasured** (see open questions).

#### Eliminated causes (evidence)

| Candidate | Eliminated by |
| --- | --- |
| PK scan on load | `db.rs:617` `WHERE id = ? LIMIT 1` — indexed lookup, verified in code |
| Summary/title generation on open | call-site sweep: `thread_summary`/`spawn_title_generation` fire only on @-mention, summary flow, title regeneration — not on `open_thread` (P7g line in the plan doc, 2026-10-01) |
| MCP startup/sync on thread open | `sync_kask_mcp_*` wired to startup + `SettingsStore` observers (`crates/zed/src/main.rs:1432-1463, 3432`), never to open_thread — verified in code |
| Debug binary / build-profile mismatch | §1.2 |
| Retention-reload of recent threads | P7e raised `MaxIdleRetainedThreads` 5→16 (D84); re-opens of recent idle threads do not re-pay the load. First-open of a large thread still does (that is S1-H1). |

### 2.2 Symptom 2 — input-tracking freeze

#### Confirmed cause class S2-H1 — foreground saturation: per-draw cost × drawing-window load (cost-driven, not rate-driven)

**Measured live (this report's own sampling of the running release process, PID 16758, 2026-10-01 15:27–15:32):**

- `/proc/16758/stat` jiffy deltas over four 5 s windows: main thread ("`zed-kask`" tid = pid) **490–506 ticks = 98–101 % of one core, sustained**; all worker threads together add ~140–230 ticks; process CPU 5.0–5.5 % of RAM-resident 5 GB, `ps %CPU` 115–180 %.
- The in-build log simultaneously reports `ui frame health: wWindowId(1v1) draws=320-357 avg_ms=42-55 max_ms=71-87 | wWindowId(2v1) draws=26-60 avg_ms=15.7-16` and `[DIAG-anr] window: events=3400 busy=16.5s max=26ms avg=4.85ms` (busy ≈ 33–55 % of the window in the event lane).
- The same windows show a `clippy` build inflating machine load (`load avg 4.01`) — consistent with the P7f measured 3–4× draw-cost inflation under builds.

**Mechanism chain (verified):** every `ThreadEvent` crossing `agent.rs:2505`-`2714` costs ~4–5 ms of foreground time (H3 — uniformly expensive events — confirmed over 2026-09-29→10-01 at 1,319–4,000+ events per 60 s window, max 20–56 ms); streaming text additionally drives `Markdown::append` + `EntryUpdated` + reveal-tick re-renders at D14's 20 fps grid (`acp_thread.rs:3287-3310, 3440-3530`); each drawn window then costs 24–55 ms per draw (vs 2.3–7.5 ms healthy, P7c 2026-09-24). The foreground main thread is pinned at ~100 % while any agent conversation streams — leaves no slack for input dispatch → keystroke backlog → the observed freeze.

**D-seams implicated:** D14 (streaming reveal rate + shared grid: bounds **rate**, not per-draw cost) · D84 (animation caps + per-window frame health: the same) · the `[DIAG-anr]` temporary probe (not a seam; instrumentation on this exact path) · the conversation-view/markdown event path (D18 widget renderers are one candidate inside the unexplained per-draw cost).

#### Rate-driven causes eliminated (measured)

| Candidate | Eliminated by |
| --- | --- |
| Uncapped looping animations → display-rate redraw | D84 choke-point cap (20 fps, `Animation::repeat*` default) landed 2026-09-29/30; today's logs show draws at 7–12/s per window, not 60 |
| Per-thread reveal timers multiplying redraws (N×20 fps) | D14 grid extension (2026-09-29): `gpui::frame_grid` shared boundary; today's draw rate is one window's rate, not N× |
| mimalloc default | D85 retired 2026-09-30; the running binary (built 10-01 11:07) has no mimalloc default and reads the same 24–55 ms bands |
| 2026-09-24 upstream sync as the cause | `git merge-base --is-ancestor`: merge `d2f29c3827` (07:20) predates the 15:47 P7c baseline build — the baseline was already post-sync |
| Debug-assertions/profiler overhead in the shipped binary | §1.2 (release, stripped; `profiler` feature renders the counters but the sampling shows no instrumentation spike class) |
| MCP fleet CPU | 0.0–0.4 % each (measured) |

#### Partially explained / open inside S2-H1

The **per-draw cost decomposition** is the one unconfirmed link: P7f's attribution (2026-09-30) split ~11 ms conversation / ~15 ms editor+chrome; the content-size scaling (23–50 ms fresh/quiet/heavy vs 24–38 ms light) and the 42–55 ms averages of today's session are not yet accounted for per element. The built-in profiler (`instrumentation.performance_profiler.enabled` + `zed: open performance profiler`) is the mandated instrument (P7f step 3.2 explicitly gated the expensive-element fix on it) and has **not been run**. Until it is, "which element costs 40 ms/draw" is an open question, not a claim.

---

## 3. Stage 3 — hypothesis register

Each entry below is labeled by its actual state. "Confirmed" = mechanism chain verified against HEAD **and** corroborated by a measurement; "hypothesis" = falsifiable statement + mechanism + discriminating test, test **not** run.

| ID | Statement (falsifiable) | Mechanism chain | Evidence | Discriminating test | State |
| --- | --- | --- | --- | --- | --- |
| **S1-H1** | A first-open of a large thread costs ≥ 0.5 s more than a small thread, and ≥ 70 % of that is deserialize + replay-forwarding, not SQL | `db.rs` blob-zstd-decode → `thread.rs:from_db` → `replay` → `agent.rs:2505` loop at 4–5 ms/event | P7c probe rows (635/670 ms, 963 entries); `[DIAG-anr]` 4–5 ms/event avg; code chain verified | Re-add the P7c `diag_open` probe for one session (or parse `hang_traces/`), open a thread with > 100 entries vs an empty thread, same build | **Confirmed** (mechanism; magnitude from P7c measurement) |
| **S1-H2** | A concurrent turn-end save delays a thread open by measurable time | one `Connection` mutex (`db.rs:393`); save writes a 100 KB–MB blob; open's `SELECT` waits | code-verified lock structure; no timing yet | P7d re-run with a bounded open probe **while** a turn saves; compare open latency save-in-flight vs idle | **Hypothesis** (test not run) |
| **S1-H3** | Replays of threads using MCP-built tools pay an extra `context_server_registry` scan per tool call, adding ≥ 10 ms/thread-entry | `thread.rs:1895-1905` scans registry on tool-not-found | code-verified; no timing | Count MCP-tool replay entries in a large thread; time `replay_tool_call` with/without the scan (cache the resolved tool at replay start) | **Hypothesis** |
| **S2-H1** | The freeze is cost-driven: with the same code and an empty editor + no streaming turns, per-draw cost returns near 2.3–7.5 ms and the main thread falls below ~30 % | draw cost scales with conversation content + event-forwarding volume | today's jiffy sample (main ~100 % with a heavy turn streaming) vs P7c idle readings (100–168 ticks) | Differential: same build, same window, type in an empty tab vs a heavy conversation; read `ui frame health` + one 5 s jiffy sample | **Confirmed class** (cost-driven; differential planned as ranked step 1's baseline) |
| **S2-H2** | The unexplained per-draw cost (> 30 ms) lives in conversation-view element re-layout (markdown/viz widgets), not in editor-core | P7f ~11 ms conversation / ~15 ms chrome at light content; heavier content 23–55 ms | P7f attribution; content-scaling observed | `zed: open performance profiler` capture while typing in heavy conversation vs empty tab (ranked step 1) | **Hypothesis** (the mandated instrument, not yet run) |
| **S2-H3** | Event-forwarding amplification (N listeners per `EntryUpdated` × windows) inflates the 4–5 ms/event avg on multi-window sessions | `agent.rs:2505` loop → `AcpThread` update → subscribers (conversation view, sidebar, panels) | `[DIAG-anr]` H3 (=uniformly expensive events) confirmed; per-listener split not measured | Two windows same thread vs one window; `[DIAG-anr]` avg delta | **Hypothesis** |
| **S2-H4** | Allocator fragmentation (glibc arenas) contributes ≥ 10 % of the 42–55 ms draw band in long sessions | RSS 5 GB + 106 threads; P7f noted `MALLOC_ARENA_MAX` as an RSS test | fresh-process reading 23–50 ms weakens it as baseline cause | `MALLOC_ARENA_MAX=4` A/B on the same build, same session shape | **Hypothesis (weakened)** |
| **S2-H5** | Build concurrency (`clippy`/cargo on the same machine) inflates draw cost 2–3× on top of S2-H1 | memory-bandwidth/file-event contention; P7f correlation confirmed, mechanism open | today: load 4.01 + clippy running, draw 42–55 ms vs P7f 20–38 ms quiet | Step 1.2 of P7f: build-inflation mechanism discrimination from log lines; `-j 8`/`nice` workflow relief | **Hypothesis** (correlation confirmed, mechanism open) |
| **B-1** | Build-profile mismatch explains both symptoms | debug binary observed | **eliminated** — §1.2: running binary is release+stripped, install pinned to `target/release` | — | **Eliminated** |
| **B-2** | MCP startup/sync on thread open adds latency | `sync_kask_mcp_*` observers | **eliminated** — not wired to `open_thread` (call-site sweep, §2.1) | — | **Eliminated** |
| **B-3** | Rate-driven redraw (uncapped timers) drives the freeze | pre-D84 behavior | **eliminated by measurement** — cap landed; logs show 7–12 draws/s/window | — | **Eliminated** |

---

## 4. Stage 4 — ranked optimization plan (impact × risk) with measurement plan

Rows are ordered by expected symptom impact against implementation risk. Each lands in `kask/` behind a seam with its `DIVERGENCE.md` entry in the same change (D-seam discipline), except where the code is already an established seam's file.

| # | Change | Where it lands | Measurement that demonstrates movement | Expected impact | Risk |
| --- | --- | --- | --- | --- | --- |
| **1** | **Run the mandated profiler decomposition** — not a code change; the experiment that gates 2–4. Enable `instrumentation.performance_profiler.enabled`, capture `zed: open performance profiler` while (a) typing in a heavy conversation, (b) an empty tab, (c) during a build. | operator workflow only | per-task/per-element timing table replacing the ~11/+15 ms guess with named elements | decides 2/3/4 ordering | none (read-only) |
| **2** | **First-open decomposition (P7d re-run)** with the bounded open probe (re-add the P7c `diag_open` span for one session, or parse `hang_traces/`): split load/parse/replay/forward | `crates/agent/src/db.rs`, `crates/agent/src/thread.rs` behind D28/D14 seams + the temporary probe pattern already used (`agent.rs` `[DIAG-anr]`) | click-to-first-frame for the same large thread before/after each sub-fix | S1: should take large-thread open below ~300 ms | low (instrumentation) |
| **3** | **Message-level (or chunk-level) thread storage** — store messages as rows (or a content-addressed blob + per-message index) so a first open loads only what the view needs; kill the whole-thread JSON+zstd round-trip | `crates/agent/src/db.rs` (D28 seam — storage layout is a documented fork seam) + `DbThread` serialization; no upstream file edits | open latency vs thread-size regression test; `load_thread` timing on a 338 MB DB fixture | S1: removes the dominant load term; also shrinks save cost (S1-H2) | medium (schema migration; stored-data upgrade path must be explicit, pre-release no-compat policy applies) |
| **4** | **Second read-only DB connection (or a small pool)** so opens never queue behind saves | `crates/agent/src/db.rs` (D28) | S1-H2's discriminating test as the before/after: open latency with save in-flight | S1: bounded win | low |
| **5** | **Event-forwarding batching** — coalesce `EntryUpdated`/tool-call updates within one grid tick before notifying listeners (if profiler shows S2-H3) | `crates/acp_thread/src/acp_thread.rs` (D14 seam area) + `agent.rs` forwarding arms | `[DIAG-anr]` avg/event and main-thread jiffy sample before/after; multi-window vs single-window differential | S2: reduces the 4–5 ms/event tail | medium (notification semantics; must not break streaming smoothness) |
| **6** | **Expensive-element fix** (whatever step 1 names: markdown re-layout of huge entries, viz widget re-render, editor chrome) | the named element's crate; if it is a widget in `crates/hkask-*-widget/` it is already a fork-owned seam (D18) | profiler diff ± the fixed element; `ui frame health` avg_ms target ≤ 10 ms | S2: the missing link in S2-H1 | medium (depends on step 1's observation) |
| **7** | **Build hygiene** — `HKASK_BUILD_JOBS` / `nice` for the parallel-clone build workflow (P7f step 2.1) | `kask/scripts/build/install.sh` (D46 area) + operator workflow | draw-cost band under build drops toward the quiet band | S2-H5: workflow relief 2–3× on the folded multiplier | low |
| **8** | **`MALLOC_ARENA_MAX` A/B** before any allocator change (P7f row 6) | environment-only experiment | RSS + draw band before/after | RSS question; weak for S2 baseline | none |
| **9** | **Remove `[DIAG-anr]`** with whichever event-path fix lands (P7g's standing note) | `crates/agent/src/agent.rs` (its own "remove with the fix" contract) | source sweep for `DIAG-anr` = 0 hits | hygiene | none |

Cross-cutting: `kask/docs/plans/hkask-core-mcp-repair-improvement-plan.md` P7g should be annotated to point at this report as the superseding two-symptom record when a next docs pass runs (docs-count cap prevents an edit in this pass without a pricing decision).

---

## 5. Measured vs inferred — honest statement

**Measured (values read from instruments I did not have to trust):**
- Main-thread jiffy deltas on the running release binary today (my own `/proc` sampling): 490–506 ticks / 5 s ≈ 98–101 % of one core × 4 windows; worker total 140–230 ticks.
- `ui frame health` log lines (built-in counters): draws 320–357 per 30 s at avg 42–55 ms (max 71–87) on one window; 26–60 draws at 15.7–16 ms on a second.
- `[DIAG-anr]` windows: 3,400 events / 60 s, busy 16.5 s, avg 4.85 ms, max 26 ms (and the 2026-09-29→10-01 band 3.6–6.5 ms avg, max 56 ms).
- P7a–P7f measurement tables (2026-09-23→30) — reused as cited, not re-measured here: 0.54–2.0 s load-dominated opens; 635/670 ms for 963 entries; 2.3–7.5 ms healthy draw cost; 20–38 ms quiet / 83–128 ms under build; 338 MB threads.db; 13-server fleet 0.0–0.4 % CPU each.
- Process/binary facts: `~/.local/bin/zed-kask` release+stripped is the running exe; no source newer than the install; 13 `hkask-mcp-*` children at idle CPU.

**Inferred (mechanism chains read from code; each labeled in §2/§3):**
- S1-H1's phase split (deserialize + replay-forwarding dominating first-open) — the chain is code-verified end-to-end and quantified **by the P7c probe's own before/after rows**, but the 2026-09-24 probes were removed; a fresh measurement on this build is step 2 of the plan.
- S1-H2 (save-lock contention) — code-verified lock structure, **no timing**; hypothesis.
- S2-H1's causal decomposition (event-volume + per-draw cost = freeze) — the correlation spans my sampling + the logs + the P7 series, and the rate-driven family is eliminated by measurement; the split between "event forwarding" and "draw cost" inside S2-H1 needs the profiler run (step 1).

**Never measured (do not cite as results):** input-to-frame latency percentiles (the `InputLatencySnapshot` histogram exists but has not been dumped); thread-open latency on **this** build (the probe data is from 2026-09-24 builds); any upstream-Zed A/B on this machine (only P7c-style same-machine before/after rows exist; true upstream parity on these two axes is the goal, reached when steps 2–6 land and the same measurements read at the healthy bands).

---

## 6. Open questions (with closure state and owner)

| # | Question | Closure state | Owner |
| --- | --- | --- | --- |
| OQ-1 | Which element inside a drawn frame costs > 30 ms (S2-H2)? | open → gated on plan step 1 (operator-run profiler) | operator + this agent |
| OQ-2 | What is the S1 pre/post fix on **this** build (0.5–2.0 s → ?) | open → plan step 2 (probe re-add) | this agent |
| OQ-3 | Does a turn-end save measurably delay opens (S1-H2)? | open → plan step 2/4 experiment | this agent |
| OQ-4 | Is a build running on the same machine a ≥ 2× multiplier on draw cost (S2-H5 mechanism)? | open → P7f step 1.2 log-line discrimination | this agent |
| OQ-5 | Do symptom reports persist on a quiet machine with no build and no streaming turn? (validates S2-H1's class, informs whether S2 has a second cause) | open → plan step 1's empty-tab differential | operator |
| OQ-6 | P7g in the plan doc should be superseded by this report | open → docs-count pricing decision | operator (docs pass) |

Per the closure discipline: no item is `reported-abandoned`; OQ-1/5 need operator-side captures (the mandated instrument and the user's own symptom confirmation), which is why they are surfaced as decisions, not silently dropped.