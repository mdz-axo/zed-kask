---
title: "media panel — bug 1 (transcript-seek playback stop) continuation prompt: the live walk"
audience: [agents, developers]
last_updated: 2026-11-07
version: "1.0.0"
status: "Active"
domain: "Media panel / UI"
mds_categories: [lifecycle, domain]
---

# media panel — bug 1 (transcript-seek playback stop) continuation prompt: the live walk

**Handoff target:** any agent with repo access and no conversation history.
**Composed:** 2026-11-07, from the composition-review thread's close-out. Tree at
compose: HEAD `9a7cfc71d0` (origin/main == HEAD, tree clean, everything pushed);
every fact below was checked against the tree at compose time — re-verify at
pickup, the tree moves (multiple streams are active).

## Read first (required, in order)

1. The worktree-root `.rules` (the only `.rules` that loads; it is binding).
2. `kask/docs/diataxis/media_panel/reference.md` v3.5.x — the "Transcript
   surface" and "Playback and layout contracts" sections (the landed
   contract; its citations were re-anchored 2026-11-07).
3. `kask/docs/research/media-panel-composition-interaction-review-2026-11-06.md`
   — the review that opened this arc (§3 the F-findings, §6 the re-runnable
   checks including the citation ladder).
4. The board "Media panel transcript bundle"
   (`8820ae83-a57f-45d6-9caf-23320e687ca8`): S1–S6 done (S6 verified with
   evidence on the card); S7 in_progress (the S7 stream's panel half).

## Mission

Bug 1 is the ONLY open item from the transcript-bundle thread. The operator's
live report (2026-11-06): "when you toggle forward the video it toggles
forward the transcript and then the video player disappears and the play
stops" — on the video-keyed transcript (gallery asset
`82d8f9b8-e87c-4cdd-8b1a-84581c6571b2`, transcript `fd6794f2`, 8,704 words,
`original` alignment, 62.3 minutes, the mp4 at
`~/Documents/zk-data/media-mcp/generated/08d8d962-91bd-4ff3-a2b2-e9599987a487.mp4`).

Two landed fixes have since changed the surface materially:

- `936ba51610` — the transcript row's img clamp (`.max_h_full()`): the row
  previously showed only the top ~third of the video with NO transport
  controls visible (the report was made against that broken control
  surface — the operator had no working play/seek controls in the row).
- `62c74212f0` — the audio output device defers to the first play/seek (the
  load is decode-only; headless machines load and land pre-play seeks).

The walk: re-test against a binary built at/after `62c74212f0` — the FIRST
binary where the transcript row shows the whole video AND the play/seek
controls inside the row.

Two outcomes:

(a) **The seek behaves** (all falsified mechanisms below say it should): bug 1
    was the broken-control-surface entanglement. Record the resolution (one
    dated sentence in reference.md's Transcript surface section), report, and
    close — no card was ever opened for bug 1 (the in-thread diagnosis
    carried it); the operator adjudicates at the algedonic review.
(b) **Play genuinely stops**: the panel-side interplay is the remaining lead.
    Build the panel-level scripted probe (below) red-first, fix at root,
    re-run gates.

## The falsified-hypothesis ledger (do NOT re-derive — every entry carries its receipt)

Every scriptable mechanism for bug 1 was tested green in the closing session
(2026-11-06/07), against the operator's REAL recording:

1. **The 1x deep seek is healthy.** A tagged probe loaded the real 62-min
   mp4, played, sought to 5/6 duration: `error=None`, `playing=true`
   throughout, landed at the target in 365ms. The worker's
   seek-error path (`video_decoder.rs`'s Err arm → pause + `Failed`
   event → the widget's "Media error" state) never fired.
2. **The 2x speed + deep seek is healthy** (the speed-button hypothesis,
   falsified 2026-11-07): the same probe at 2x (three CycleSpeed events):
   `error=None`, `playing=true`, position 3115.80s — the 5/6 target —
   instantly. The opus-in-mp4 rate path is not the failure.
3. **No suspension path fires for the panel's widget.** `mark_visible` (the
   only setter of `visibility_managed`) is called ONLY by `hkask-viz-core`
   (the chat-thread embedding); the panel's widget stays unmanaged, so
   `tick_playback`'s 250ms visibility gate never fires there. The panel's
   `suspend_media_widget` callers are tab-switch, gallery change, and
   selection change — none on seek or scroll.
4. **EOF is handled**: `decode_frame_at` returns
   `Ok(DecodeOutcome::EndOfStream)` at decoder exhaustion — not an error.
5. **The "player disappears" half is expected virtualization**: the
   transcript's player row is item 0 of the workspace's virtualized
   `gpui::list`; after a forward seek the follow-along scrolls it out of
   the viewport. Playback should continue audibly — the mp4 carries an
   opus audio track.

What remains untested: the panel-side interplay — the follow-along scroll +
the virtualized list + the live widget + the S6 preview machinery
(`ReelPreview`/`step_reel_preview`) interacting with a live seek. No
panel-level scripted probe of seek-while-playing exists yet.

## The discriminating questions (route the operator's live report on their answers)

- Was "toggle forward" the **seek slider** or the **speed (x) button**?
- Did a **"Media error: ..." line** appear where the player was? (The widget
  ledger falsifies both seek paths at 1x and 2x, so a live Media error would
  point at a panel-driven path the widget probes never reached.)
- Does the **audio continue** after the player row scrolls out of view?
  (Separates "play stopped" from "controls left the viewport".)

## The panel-side probe design (outcome (b) only)

A scripted panel-level interaction test in `crates/media_panel`'s test
module: construct the transcript workspace over a real widget (the machinery
is demonstrated by the committed `reel_preview_plays_spans_in_composed_order`
pin — it loads real audio, arms the preview, and drives scripted seeks),
play the committed `crates/media_panel/test_data/reel-preview.wav` (or the
real mp4), drive `seek_workspace_transcript` while playing, and assert: the
widget stays playing; the follow-along marks; the player row's
virtualization does not stop playback; the armed preview survives a manual
seek. Red pin first per the discipline.

## Verified ground truth (2026-11-07, re-verify at pickup)

- **Gates at `62c74212f0`**: `cargo test -p hkask-media-widget` 73 passed +
  1 ignored (72 + `audio_load_defers_the_output_device`); `cargo test -p
  media_panel` 132; `./script/clippy` both crates exit 0.
- **The loaded-runner flake class**: `speed_two_x_advances_video_position_at_double_wall_clock`
  flaked ONCE in a full-suite run under load — green twice in isolation.
  Re-run in isolation before blaming a change (the .rules rule; the wall-clock
  video pins are the documented family).
- **CI**: `reel_preview_plays_spans_in_composed_order ... ok` on the 19:42Z
  2026-11-07 run — the three-run CI red (the headless audio load) is closed
  and CI-validated. Remaining CI red at compose: the duplication ratchet
  (5663 > 5635) — other streams' earning-commit debt; adjudicate whose
  before reacting (`gh run view <id> --repo mdz-axo/zed-kask --log-failed`;
  jscpd report names are path SUFFIXES — resolve by unique-suffix match).
- **The build chain**: verify at pickup before interpreting any live
  observation — the process's exe mtime vs the fix commit time vs the
  process start (`pgrep -f zed-kask` → `readlink /proc/<pid>/exe` →
  `stat -c %y` → `ps -o lstart=`). A re-test against a stale binary
  reproduces fixed bugs and misleads; this pattern was verified live twice
  in the closing thread.
- **Bug 2 is FIXED and pinned**: `.max_h_full()` on both imgs
  (`.max_h_full` is load-bearing — gpui's img.rs injects the frame's
  intrinsic aspect ratio into the style whenever none is set, and the
  width-derived height OVERRIDES height percentages; without the clamp a
  16:9 frame laid out 449px tall inside a 100px area). Pinned by
  `transcript_row_fits_frame_and_transport`; do not re-open without
  falsifying the pin.
- **Headless reproduction from the shell** (the widget crate forbids unsafe
  code, so no in-process env games): `ALSA_CONFIG_PATH=/nonexistentalsa
  cargo test -p media_panel <pin>` reproduces a headless runner's
  device-open failure deterministically — it is how the CI red was
  reproduced and then shown green.

## Binding process directives (abridged — the .rules is authoritative)

- Machine checks before judgment; the oracle re-runs against the change
  claiming it — no inherited green, no claim without its oracle re-run.
- Shared tree: pathspec commits only; staged-index check before every
  commit; numstat verification against the measured pre-commit diff; no
  working-tree-wide mutations; other streams' files and unpushed commits
  are untouchable; never push (the operator pushes).
- Probe discipline: every diagnostic test tagged `[DIAG-xxxx]` and DELETED
  before any commit — at close, `rg -c '\[DIAG-' <touched files>` must
  return nothing. The closing thread used and removed two (SEEKDEEP,
  SPEEDSEEK); their bodies are in the thread history, not the tree.
- GPUI traps: timers via `cx.background_executor().timer` (never
  `smol::Timer`); no `block_on` foreground; `AsyncApp` is not `Send`;
  `#![forbid(unsafe_code)]` holds in the widget crate.
- Citation rule: any line-shifting edit to a cited file re-verifies its
  citations in the same change (the ladder lives in the review report §6;
  reference.md's anchors are verified 2026-11-07 — keep them so).
- Kanban: one state-mutating call per task per batch; findings route to the
  board and the operator — no new cards without the operator's adjudication
  (bug 1 has none; this prompt carries it).
- Goal governance: no goal operations from this work; the successor goal
  `5960192f`'s phase gates (criteria S5/S6/S7) are the goal flow's and the
  operator's.

## First actions at pickup

1. Verify the state: `git --no-optional-locks status --short`;
   `git --no-pager log --oneline -5`. Has the operator reported the walk?
   (This prompt may be picked up WITH the report in hand — route on the
   discriminating questions above.)
2. Verify the build chain before interpreting any live observation.
3. If the walk is green: record the resolution note, report, close.
4. If the walk shows a genuine stop: the answers route the panel-side
   probe; build it red-first, fix at root, gates re-run, and route the
   findings per the board discipline.
