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

Each entry below is labeled by its actual state. "Confirmed" = mechanism chain verified against HEAD **and** corroborated by a measurement; "hypothesis" = falsifiable statement + mechanism + discriminating test, test **not** run. Round-2 register additions — **B-1R** and the **H-ECO** series — live in §7.4; this table is the round-1 record.

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

---

## 7. Round 2 (2026-10-02) — new-build re-baseline, binary provenance, and the H-ECO register

**Context.** After the round-1 write-up the code was rebuilt/restarted and the operator reported performance **worse**. This section is the round-2 Stage-0 re-baseline on the new build, the binary-provenance finding, the regression-window source delta, and the formalized H-ECO register (the operator's 2026-10-02 hypothesis). Round-1 §1–§6 stand as the historical record; §7.5 supersedes §4's plan ordering.

### 7.1 Binary identity and provenance — the running binary is not a product of this tree's build path

| Fact | Evidence |
| --- | --- |
| Running binary: `~/.local/bin/zed-kask`, installed 2026-10-02 06:23; app PID 1090835 started 06:23:23 | `stat`; `ps`; `/proc/1090835/exe` |
| It self-identifies: `starting zed version 0.40.0+dev.e5a8c29e6c9c0898836eb6e51702439fdd3ddc96, sha e5a8c29` | `Zed-Kask.log.old` 06:23:23 |
| Commit `e5a8c29e6c` ("Prohibit repository fragmentation", 10-01 20:56): the whole Oct-1 churn is in the binary; the Oct-2 06:43+ commits are not | `git log` |
| **Debug assertions ON in the running binary**: startup logs `WARN [zed_kask::reliability::hang_detection] debug build, only reporting hangs longer then 5s`; the gate is `cfg!(debug_assertions)` (`crates/zed/src/reliability/hang_detection.rs:32-57`) | log + source |
| Consequence (a): a uniform hot-path multiplier (overflow checks + `debug_assert`/`assert` arms on every operation); consequence (b): the hang detector is blind below 5 s and the frame-budget incident threshold is 100 ms (vs 24 ms in release builds) — **absence of hang reports this session is not evidence of absence** | `hang_detection.rs:32-53` |
| **Not an install.sh product**: install.sh builds `cargo build --jobs "$jobs" --release --package zed` (`kask/scripts/build/install.sh:245`) and sets no `ZED_COMMIT_SHA`/`ZED_BUILD_ID`/`RUSTFLAGS` (grep over install.sh: zero hits); a local build prints `sha unknown` (`crates/zed/src/main.rs:386-417`) and `[profile.release]` (`Cargo.toml:1254-1262`) enables neither debug assertions nor an embedded sha | install.sh, Cargo.toml, main.rs |
| **Not built in this tree**: `target/release/zed-kask` and its hardlink `deps/zed_kask-a7937ef9f58b320b` are Sep 30 20:19 — the artifact round-1 measured (638 MB installed); no in-tree release build exists after that date; the new install is 674 MB | `ls target/release{,/deps}`; round-1 §1.2 |
| Install path: the download-only updater bundle (`~/.local/share/zed-kask/install/`, all files 06:23; `install-binary.sh` downloads a verified GitHub release archive and never builds). No cron/systemd trigger, no shell-history entry. **The pipeline that built the artifact is unidentified — nothing in this repo sets `ZED_BUILD_ID` (grep: only the `option_env!` read at `main.rs:386`)** | `install-binary.sh:1-136`; `crontab -l`; `systemctl --user list-timers`; `~/.bash_history` |

**Conclusion:** round-1's eliminated hypothesis **B-1 (build-profile mismatch) is un-eliminated for the new binary** — re-registered as **B-1R** (§7.4). The 06:23 update swapped a local release build (debug assertions off) for an external pipeline artifact (debug assertions on). The artifact's codegen level is not directly measured; its animation floor (§7.2) argues against opt-level 0 — the flag that *is* measured is `debug_assertions=on`.

### 7.2 New-build readings vs round-1 (same machine; loadavg 1.40 on 24 cores, no build running, 13 `hkask-mcp-*` servers at 0.0–0.3 % CPU)

| Instrument | Round-1 (Oct-1 binary) | New build (Oct-2) | Delta |
| --- | --- | --- | --- |
| `ui frame health` avg_ms, heavy streaming | 42–55 (max 71–87) | **73–104** (max 117–255) | ~1.7–2× |
| `[DIAG-anr]` avg/event | 3.6–5.4 ms (busy ≈ 33 % of window) | operator's morning 06:28–06:46: **4.7–5.9 ms** (parity), drifting to 5.9–8.9 through 07:24; this session 08:25+: **13.0–16.2 ms**, busy 33–54 s/60 s (55–90 %) | parity early; 2.7–3× under this session's content |
| Telemetry `Frame Duration Report` (instrument first read this round) | — | avg dirty→present **49 → 64 → 79 → 118 ms** across the last hour; p50 46.7→62.2; p95 86→144 | corroborates the degradation |
| Animation floor (20 fps-cap windows, ~500–590 draws/30 s) | quiet band 20–38 ms (P7f) | **23–29 ms** | within the old quiet band — the floor did not move |
| Main thread | 98–101 % of one core (jiffy, 5 s windows) | ~100 % (562 ticks/~5 s), RSS 5.4 GB, 107 threads | saturation unchanged |
| `threads.db` | 338 MB | 360 MB | +6 % |
| Within-session trajectory | 42–55 at ~4 h into session | 38.3 ms at 23 min → 104 ms at 2 h | degrades ~2.7× within the session |

**Reading.** The new binary *starts* at round-1 parity (the operator's morning session: 4.7–5.9 ms/event, 39–55 ms draws) and degrades with content; the worst windows (08:25+) coincide with this investigation's own large tool outputs landing in the conversation. Honest-measurement notes: (1) the investigator's session contaminates its own worst readings — the operator's morning bands (which reached 58.9–89.2 ms, bands round-1 never recorded) are the cleaner regression signal, though thread content differs between sessions; (2) `[DIAG-anr]` busy/avg measure wall time per event, which includes time descheduled while the foreground draws — the 08:25+ event-avg inflation is therefore partly downstream of the draw-cost inflation, not necessarily an independent per-event regression. The build-vs-content magnitude split is exactly what ranked step R1 (§7.5) measures.

### 7.3 Regression-window source delta — no hot-path candidate

`f463bce207` (Sep 30 17:19, ≈ the round-1 binary's tree state) → `e5a8c29e6c`: 26 files, +1327/−3254 in binary-linked Rust, **all background** — regulation sensor/policy (`f3f83bf021`, `301c5a29d5`, `84bc0a4462`), evolution health bridge (`b43a704c9c`), lisp builtins (`be44843324`), kanban goal ingestion (`e6c00be216`), embedding cleanup (`f9e29adc65`), strategy-evaluator deletion (`3267b2df20`). Zero changes to the draw path (gpui/editor/markdown/viz) or the event-forwarding path (`agent.rs:2505` / `acp_thread.rs`). The commit-range hypothesis has no mechanism; **B-1R (build flags) is the leading cause candidate**, with content scaling (S2-H1's confirmed class) stacking on top.

### 7.4 Register additions — B-1R and the H-ECO series

H-ECO (operator's hypothesis, 2026-10-02): *duplicated logic paths, uncoordinated shared-resource access, and uneliminated work produce divergent behavior, races, and waste; the ecosystem must be sorted layer by layer.* Sub-claims, each tested this round:

| ID | Statement (falsifiable) | Instance found this round | State |
| --- | --- | --- | --- |
| **B-1R** | The 06:23 artifact's build flags (debug assertions on release-class codegen) explain a uniform 1.5–3× hot-path multiplier vs the round-1 binary | mechanism: overflow checks + assert arms on every hot-path op; evidence: hang-detection line + gate source, `install.sh:245`, `Cargo.toml:1254-1262`, `main.rs:386-417`, no in-tree build after Sep 30 | **Hypothesis (mechanism confirmed, magnitude unsplit)** — discriminating test: R1's same-session A/B rebuild |
| **H-ECO-dup** | Same behavior implemented N times → divergent behavior, N× maintenance surface | **The two installer codepaths** (confirmed with harm): `install.sh` (source build: release, no embedded sha) vs `install-binary.sh`/`update-zed-kask.sh` (download-only: pipeline artifact with debug assertions) — both live, they produced different binaries with different flags, invisible until measured (this incident). Corroborating: the ratchet's documented prior instance ("the redraw grid existed as two private copies before consolidation", `check-duplication-ratchet.sh` header), the Oct-2 dedup series (`57e5a7f14d`, `be51a486b1`, `0b0d6c3f8b`), and the system-prompt assembly pair (canonical cached `render_system_prompt`, `thread.rs:5687/5706`, vs direct `SystemPromptTemplate` construction — the `.rules` byte-stability warning; production-bypass status not swept this round) | **Confirmed (instance-with-harm: the installer pair)**; full dependency-hierarchy review = R5 |
| **H-ECO-race** | Uncoordinated shared-resource access | (1) the binary-swap race: an uncoordinated update path replaced the running binary mid-investigation with a flag-regressed artifact; (2) duplicate `Kask MCP sync fired` + `Removing 13 raw context_servers entries` within one second at startup (settings-observer fan-out firing twice, `Zed-Kask.log.old` 06:23:24 ×2); (3) S1-H2's single shared `Connection` mutex (carried, unmeasured) | **Confirmed (instances)** |
| **H-ECO-waste** | Work not eliminated because no canonical shared path owns it | (1) regulation board-sensing polls every 10 s + tool-journal writes (`kanban_board_list`/`kanban_task_list`, 1–48 ms server-side, throughout the log) — tree-fixed at `821055ce47` (06:52) but **not in the running binary** (deployed-binary lag as a waste vector); (2) whole-thread JSON+zstd re-serialization per save/open (S1-H1/D28 — the canonical instance); (3) four background tick loops (cybernetics 10 s, metacognition 30 s, harness monitor 60 s, consolidation 300 s) — present, foreground-relevance unmeasured | **Confirmed (instances on the load path)**; foreground hot-path split gated on the profiler (OQ-1) |

### 7.5 Round-2 ranked plan (supersedes §4's ordering; §4 rows carry as R4)

| # | Action | Owner | Measurement that demonstrates movement |
| --- | --- | --- | --- |
| **R1** | **Rebuild + reinstall the local release binary via install.sh** — the B-1R splitter; also deploys the tree's already-landed fixes (`821055ce47` board-sensing skip, `57e5a7f14d`, `be51a486b1`, `0b0d6c3f8b`). Schedule-aware: the build pegs 16 jobs and inflates draw cost 2–3× while it runs (P7f) — run when the operator is not using the app | operator + agent | same-session before/after: `ui frame health` bands, `[DIAG-anr]` avg, one 5 s jiffy sample. B-1R's share is the delta that returns toward round-1's bands (42–55 ms draws, 3.6–5.4 ms/event); any residual is content/accumulation (S2-H1/S2-H4), split further by R2 |
| **R2** | **Run the mandated profiler** (round-1 step 1 — still never run) on the clean binary: heavy conversation vs empty tab vs build | operator | per-element timing table (closes OQ-1) |
| **R3** | **PM decision: which installer path is canonical** (the H-ECO-dup fix). If the download path stays, its pipeline must build with install.sh-equivalent flags and embed provenance; identify the pipeline (OQ-7/8) | PM | a future update cannot reproduce this incident; the updater refuses or flags debug-assertion artifacts |
| **R4** | Round-1 §4 steps 2–9 carry unchanged (message-level storage, read-only connection pool, event batching, expensive-element fix, build hygiene, `MALLOC_ARENA_MAX` A/B, `[DIAG-anr]` removal) | agent | as §4 |
| **R5** | H-ECO layer-by-layer: run `kask/scripts/check-duplication-ratchet.sh` (read-only mode) + the dependency-hierarchy review over the kask surface; one canonical path per behavior (the PM's stated requirement) | agent | ratchet output + dup inventory in the next findings pass |
| **R6** | Record the installer divergence in DIVERGENCE.md if R3 keeps both paths | agent | DIVERGENCE.md entry in the same change as any R3 code |

### 7.6 Open questions (round-2)

| # | Question | Closure state | Owner |
| --- | --- | --- | --- |
| OQ-7 | Who/what built and installed the 06:23 artifact? | **CLOSED 2026-10-02** — the operator's habitual `bash script/install.sh --debug` (90 `~/.bash_history` entries): root `script/install.sh` (fork-modified thin delegate) → `kask/scripts/build/install.sh --debug` → dev-profile local build at current HEAD. See §7.9 | PM (answered) |
| OQ-8 | What profile/flags does that pipeline use? | **CLOSED with OQ-7** — `--debug` → dev profile, `debug_assertions` on; **fix LANDED**: the dev-profile install route is deleted, `--fast` replaces it (§7.9) | PM (answered) |
| OQ-9 | Which pair did the PM's "two assembler codepaths" ruling (2026-10-01) name? | **CLOSED 2026-10-02** — no specific pair named; PM ruling: "there shouldn't be two assembler code paths unless agents went rogue and I didn't catch it" — duplication itself is the defect class; R5 authorized to find rogue duplicated paths (§7.9 seed inventory) | PM (answered) |
| OQ-1 | per-draw element decomposition | open → gated on R2 (profiler on the clean binary) | operator + agent |
| OQ-2/3 | fresh first-open measurement; save-lock contention timing | open → gated on R1 (probe re-add per §4 step 2) | agent |
| OQ-4 | build-inflation mechanism | open → unchanged | agent |
| OQ-5 | quiet-machine empty-tab differential | open → re-run on the clean binary (R1/R2) | operator |
| OQ-6 | P7g supersession pointer in the plan doc | open → docs pass (this doc is now the round-1+round-2 record) | operator |

Round-2 termination check: symptom 1 — mechanism carried (S1-H1, §2.1) with the regression window exonerated (§7.3); symptom 2 — cause class re-confirmed on the new build's readings **plus** a new confirmed build-flag mechanism (B-1R) whose magnitude split is R1's job; H-ECO's three sub-claims each tested with named instances (§7.4). Parity itself is not yet reached — the goal remains `continue`.

### 7.7 R1 result — the 14:19 rebuild (2026-10-02 afternoon)

**Binary identity (verified):** `~/.local/bin/zed-kask` installed 14:19:24 from commit `aec78779b3` (= HEAD at build time, 14:16); running process started 14:26; startup line `dev.aec78779b3…` (`Zed-Kask.log:1771`). The build contains the full Oct-2 range — the board-sensing journal skip (`821055ce47`; ~90K journal writes/day removed, 98 % of `reg.tool.completed` volume), the agent-loop success-repetition guard (`1d4bff6fb2`), the reasoning-runaway watchdog (`aec78779b3`), the inference IPC socket swap (`86ef3d876f`) — and the tree was clean at build time (no half-edit hazard; the round-2 findings commit `aadab9a4b3` is in HEAD, docs-only). The `reg.tool` log lines still seen are the MCP server's own `serve_inner` logging — a different surface from the journal the skip targets; the skip's live behavior is pinned by its commit's tests, and ancestry (sha = HEAD ⊇ `821055ce47`) puts it in the running binary.

**The flag did NOT change:** the startup still logs `debug build, only reporting hangs longer then 5s` (`Zed-Kask.log:1783`) — the rebuild came through the **same external pipeline**, not install.sh (in-tree `target/release/zed-kask` still Sep 30 20:19). OQ-7/8 remain open and are now **parity-blocking**: upstream ships release flags, and every rebuild of the running binary carries debug assertions — a uniform hot-path tax upstream does not have, which alone prevents flag-level parity regardless of its measured share.

**Readings (15-min session, single window dominant, loadavg ~1.9 on 24 cores, active streaming):**

| Metric | Morning artifact, early (24–37 min) | Morning artifact, degraded (2 h) | New binary `aec78779b3` (1–16 min) |
| --- | --- | --- | --- |
| `ui frame health` avg_ms | 39–55 | 73–104 | **22–35** (steady maxes 45–96) |
| `[DIAG-anr]` avg/event | 4.7–5.9 | 13.0–16.2 | **4.6–5.9** (max 9–20) |
| Main thread (jiffy / 5 s) | ~100 % (4 h avg 83.6 % CPU) | — | **~75 %** (755 ticks) |
| Telemetry dirty→present | avg 49, p50 46.7 (first report) | avg 118, p50 62.2 | avg 48.9, **p50 35.5** |

22–35 ms is the P7f healthy quiet band (20–38) — the new binary sits at the healthy end **under active streaming**, below round-1's heavy readings (42–55, taken with a build running).

**B-1R re-weighted (evidence, not assumption):** the debug-assertion flag is unchanged, yet draws dropped to the healthy band — the flag is **not the dominant driver** of the morning's degraded bands. The operator's "worse" report decomposes to: session accumulation (39→104 ms over 2 h — the S2-H1 content-scaling mechanism) plus confounds (a second live window in the morning session; workload differences). The flag remains a real defect on two independent grounds — the hot-path tax, and instrument blinding: in this binary the hang detector cannot see below 5 s and the frame-budget incident threshold is 100 ms (24 ms in release), so "no hang reports" is not evidence of absence. Fixing the pipeline is required for parity either way.

**Unattributed improvement:** the new binary starts lower than the morning artifact did at comparable age (22–35 vs 39–55). Candidates not decomposed: the ~90K/day journal-write removal, the agent-loop guards, single-window workload, session-age mismatch. No mechanism claim is made.

**Sharpened next measurements:**

1. **R2 as an early-vs-late differential** — capture the profiler's per-element timing at ~15 min (healthy, 22–35 ms) and again at ≥ 2 h in the *same* session; whatever grows names the accumulating per-draw cost. This closes OQ-1 directly and is now the top-ranked action.
2. **Accumulation re-check** — read `ui frame health` + `[DIAG-anr]` at ~2 h session age on this build. If the morning trajectory (→104 ms) reproduces, accumulation is confirmed build-independent (S2-H1/S2-H4); if not, a range commit fixed it and bisection is cheap.
3. **True flag A/B still pending** — a local install.sh release build (in-tree `target/release` untouched since Sep 30). Schedule when the operator is not using the app (16-job build inflates draw cost 2–3× while running, P7f), or supersede it by fixing the pipeline's flags (R3/OQ-8 — the parity-required path).

**Learning banked:** two rebuilds in one day, both through the unidentified pipeline with the same flag defect — every "rebuilt and restarted" must begin with the startup-line check (sha + debug-build warning) before any performance reading is trusted. Binary identity precedes code attribution.

### 7.8 R1 follow-up — mid-session trajectory, profiler enabled, ratchet at baseline (2026-10-02 15:10)

**Mid-session accumulation sample (session age ~45 min, 15:04–15:10, active streaming):** draws avg **20.7–33.5 ms** (maxes 53–100), `[DIAG-anr]` avg **5.56–5.74 ms/event** — flat at the healthy band. The morning artifact read 60–89 ms by ~50 min of age; this build has not begun that climb. The discriminating test remains the ≥ 2 h checkpoint (session started 14:26 → checkpoint ≥ 16:26): if the morning trajectory (→ 104 ms) reproduces, accumulation is build-independent (S2-H1/S2-H4); if not, a range commit fixed it and bisection is cheap.

**Profiler instrument now live:** the operator's settings carry `instrumentation.performance_profiler.enabled: true` (added 15:02; diff vs `settings.json.bak-profiler` shows the addition) — round-1's "mandated instrument is off" condition is cleared. The R2 early-vs-late differential is now runnable: `zed: open performance profiler` once now (healthy band) and once at ≥ 2 h session age, same session. If the panel shows no spans, one app restart is needed for the setting to take effect.

**hang_traces/ is empty** (0 files) — consistent with the debug-assertions hang detector's 5 s blindness (`hang_detection.rs:32-42`): the instrument cannot record the sub-5 s stalls that constitute symptom 2. No fresh thread-open data from that source; OQ-2 stays gated on the probe re-add (plan §4 step 2).

**Duplication ratchet (R5 first half):** `check-duplication-ratchet.sh` reads **2820 clusters, at baseline** — no net-new duplication since the baseline commit (`b25cb266e3` tightened it 13:28). This is the drift check, not the inventory; the cluster-size extraction + dependency-hierarchy review is the R5 dedicated pass, calibrated by the PM's OQ-9 answer (which pair the "two assembler codepaths" ruling names).

### 7.9 Round-2 closure — pipeline identified, register corrected, R5 seeded (2026-10-02 15:30)

**OQ-7/8 CLOSED — the "unidentified pipeline" is the operator's own habitual install command.** `bash script/install.sh --debug` (90 entries in `~/.bash_history`) → root `script/install.sh` (fork-modified 37-line thin delegate to `kask/scripts/build/install.sh`) → `--debug` sets `HKASK_BUILD_TYPE=debug` → the else branch (`install.sh:253-261`) builds zed + all MCP servers on the **dev profile** (no `--release`) → installed from `target/debug/` (`install.sh:294-295`), stripped on copy (`:316`; 2.1 GB → 674 MB). Evidence: `target/debug/zed-kask` mtime 14:19 = the install minute; both Oct-2 binaries match local HEAD at their install times (`e5a8c29e6c` at 06:23, `aec78779b3` at 14:19); `crates/zed/build.rs:49-67` embeds the sha via a `git rev-parse HEAD` fallback — why a local build self-identifies.

**Corrections to §7.1 (owned, this round):** three round-2 rows were wrong — (1) "not an install.sh product" (it is: the root delegate forwards `--debug`); (2) "not built in this tree" (built in-tree into `target/debug/` — I checked only `target/release`); (3) "nothing in this repo sets `ZED_BUILD_ID`" (`build.rs:73` does; my grep scoped `crates/zed/src` and missed the crate-root `build.rs`). The GitHub-releases 404 was the falsifier I noted but did not act on — the download-updater theory should have died there.

**B-1R mechanism COMPLETE:** `--debug` → dev profile → `debug_assertions` on. Round-1's Sep-30 baseline was a release install; both Oct-2 binaries are debug installs of the same command. The flag is operator-selectable: `bash script/install.sh` (no flag) is the parity-flags rebuild and the true flag A/B. Magnitude stands as re-weighted in §7.7 (the 14:19 debug binary reads at the healthy band — the flag is a minority draw-cost share but remains parity-blocking and blinds the hang detector below 5 s).

**H-ECO-dup register row corrected:** the two installer ENTRY points are a redirect (single implementation — correct design, not duplication). The live harm is the **`--debug` footgun**: a flag on the canonical path that silently ships a dev-profile daily-driver. The source-vs-download duality (`install.sh` vs `install-binary.sh`) remains two maintained paths with different flag surfaces — structurally H-ECO-dup, no live harm (the download path 404s; no releases are published).

**OQ-9 CLOSED — PM ruling 2026-10-02:** no specific pair named; duplication itself is the defect class ("there shouldn't be two assembler code paths unless agents went rogue and I didn't catch it"). R5 is authorized to find rogue duplicated paths, essentialist lens, layer by layer.

**R5 seed inventory** (jscpd 5.4.0, min-tokens 70, the ratchet's exact scan set — 272 paths; regenerate with the `scan_set` block from `check-duplication-ratchet.sh:65-72` + `--reporters json`): **2820 clusters** (2813 rust, 7 bash). Largest by duplicated lines: `git_panel.rs` self-dup 94l; `tests/mod.rs` self-dup 90/80/73/64l; `provider/anthropic.rs` ↔ `anthropic_compatible.rs` 88l; `sidebar_tests.rs` self-dup 71/56/56/53l; `tools/evals/edit_file.rs` ↔ `write_file.rs` 65l; `hkask_mcp_media.rs` self-dup 58l; `tools/copy_path_tool.rs` ↔ `move_path_tool.rs` 56/53l; `agent_panel.rs` self-dup 54l; `settings_store.rs` self-dup 53l. **Triage rule for the R5 pass:** fork-owned duplication (kask/ surface + fork-introduced pairs in diverged files) is actionable; upstream-inherited duplication inside diverged files (the provider pair, panel/test self-dups) is upstream's cost — not fixed speculatively (D-seam discipline).

**R3 decision (presented to PM, 2026-10-02):** the `--debug` footgun. (1) Drop the flag when rebuilding — release profile, parity flags, slowest build (thin LTO; sccache + 16-job cap help). (2) Add a `--fast` option on the existing `[profile.release-fast]` (`Cargo.toml:1275-1279`: lto=false, codegen-units=16, inherits release → `debug_assertions` off) — fast builds WITH parity flags, serving the iteration-speed need that likely drove `--debug` (90 uses). (3) Guard `--debug` with a loud warning naming the cost (dev-profile daily-driver: slow hot paths, hang detector blind below 5 s, 100 ms frame-budget threshold). Agent recommendation: **2 + 3**.

**R3 IMPLEMENTED, then CORRECTED (PM essentialist ruling 2026-10-02: "clean it up, don't layer"):** the first implementation (commit `bdf179878d`) kept `--debug` behind a warning and added `--fast` — additive, exactly the layering the PM rejects (P5.5: a change that only adds is not done). The corrected fix **deletes the dev-profile install route**: no `--debug` flag (it falls to the generic unknown-option error, whose help lists `--fast`), `HKASK_BUILD_TYPE` validated to `release|release-fast` at parse time (the env route can no longer select dev — and cannot silently substitute fast for an invalid value), the warning block gone with its subject, and the gate now pins the **absence** of a dev-profile route (`HKASK_BUILD_TYPE="debug"` / `target/debug` greps) instead of pinning a warning's presence. `--fast` fully replaces `--debug`'s one legitimate use — debugger workflows — because `[profile.release-fast]` carries `debug = "full"` (`Cargo.toml:1275-1279`): breakpoints work; what it drops is only the `debug_assertions` runtime tax. A dev-profile build remains available the way it always was for repo developers — plain `cargo build` — because an install is not a dev workflow.

### 7.10 Parity rebuild verified — both instruments at the healthy band (2026-10-02 18:15)

**Binary identity (the §7.7 discipline, applied):** installed 17:03:30 from `target/release-fast` (mtime match — the `--fast` route), 443 MB, running sha `3f6f844` = HEAD at build time; the startup line has **no `debug build` warning** — release-class flags, `debug_assertions` off, hang detector unblinded (100 ms hang / 24 ms frame-budget thresholds). App restarted 18:13:51.

**The flag A/B, same day, same machine (confound owned below):**

| Reading | Debug build `aec78779b3` (14:19 install) | Release-fast `3f6f844` (17:03 install) |
| --- | --- | --- |
| Draws avg, early session | 39–55 ms | **6.5–6.9 ms** (maxes 21–49) — the P7c healthy band, first time since 09-24 |
| `[DIAG-anr]` avg/event | 4.6–5.9 ms → 7–9 ms by 2 h | **35 µs** (max 251 µs) |
| Trajectory to 2 h+ | 16:40 collapse: draws avg 194.8 ms, **max 12,855 ms**, then 21 draws avg 1,143 ms; session shut down 16:41:05 | flat through 18:15 (checkpoint at ~20:13 pending) |

**Confound (not split, and now academic):** the range `aec78779b3..3f6f844` includes other streams' runtime removals — the D14 shared redraw-grid facility (`frame_grid.rs` deleted, `animation.rs` −136 net; `3f6f844`) and the reasoning-runaway watchdog (`5338167d11`) — so the improvement is flags × commits. The commit share is likely the larger one: even round-1's **release** build read 3.6–5.4 ms/event and 42–55 ms draws, and this build reads 35 µs / 6.5–6.9 ms. Both fixes are landed; the split would need a release-fast build of the old commit and serves no open decision.

**The 16:40 collapse (symptom-2's extreme, captured):** the debug session's terminal event was not the morning's gradual climb (39→104 ms) but a sudden collapse at 2 h 14 m — one 12.8-second draw, then 1.1-second-average draws until shutdown. Two degradation shapes on the same debug build; the session is gone, and the grid facility it still contained is the removed suspect. Not further diagnosable from logs.

**Hang detector's first honest data:** 17 `hang detected` entries in the new session (release thresholds) — a startup cluster (18:13:53–54) plus periodic singles through 18:16. With draws averaging 6.5–6.9 ms these are the >100 ms tail (startup, MCP sync, thread opens) — the residue instrument for symptom 2, now visible for the first time. Main thread read 94 % during the startup+streaming window (not a settled-state reading; not comparable to 2 h-age samples).

**Open-question re-basing:** OQ-1 (per-draw >30 ms element) is **overtaken by the readings** — no such element exists in the current 6.5–6.9 ms bands; the profiler captures are now confirmation, not decomposition. S1-H1's dominant measured term (per-event forwarding, 3.6–5.4 ms) now reads 35 µs — thread-open latency should collapse accordingly; OQ-2's measurement is the operator's next thread-open (the P7c probes are gone and `hang_traces/` is empty). The accumulation checkpoint re-bases to this build: session started 18:13 → read at ~20:13. The D46 DIVERGENCE addendum landed with this pass (the concurrent edit cleared).
### 7.9 OQ-2 unblocked — the `[DIAG-open]` probe is live (2026-10-02 evening)

Plan §4 step 2 executed: the bounded first-open decomposition probe is in the tree (`crates/agent/src/db.rs` `load_thread` + `crates/agent/src/agent.rs` `load_thread`/`open_thread`), tagged `[DIAG-open]` with the same removal contract as `[DIAG-anr]` (remove with the S1 fix). One line set per real open (cached sessions short-circuit):

- `[DIAG-open] db_load thread_id=… lock_wait_ms=… read_parse_ms=… blob_bytes=…` — the S1-H2 lock-wait (a concurrent save holding the one shared connection) split from the S1-H1 read+zstd+JSON round-trip, plus the blob size for content-scaling context
- `[DIAG-open] from_db thread_id=… entries=… from_db_ms=…` — entity construction, including the saved-model re-resolution (`LanguageModelRegistry::select_model`) the phase hides
- `[DIAG-open] open thread_id=… db_total_ms=… replay_ms=… forward_ms=… ready_ms=…` — the phase summary; forward is the same per-event path `[DIAG-anr]` measures, so the two probes cross-check

The measurement is operator-gated on the next real open of a large thread (readings land in `Zed-Kask.log`); the S1-H2 discriminating test — open a thread while a big turn's save is in flight — is now runnable. Validation: `cargo check -p agent` clean, db tests 18/18, the D28 override pin green, clippy green; the one failing agent test (`test_reasoning_runaway_watchdog_aborts_thinking_only_stream`) is another stream's uncommitted in-flight work, untouched by this change.
