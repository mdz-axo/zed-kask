---
title: "Continuation prompt — media panel transcript bundle: S4–S7"
audience: [agents, developers]
last_updated: 2026-11-05
version: "1.0.0"
status: "Active"
domain: "Media panel / UI"
mds_categories: [lifecycle, domain]
---

# Continuation prompt — media panel transcript bundle: S4–S7

**Handoff target:** any agent with repo access and no conversation history.
**Composed:** 2026-11-05, from the pickup session (tree verified clean at HEAD
`fe9ab1f696`; running build verified chain-complete: HEAD commit 20:30:58,
exe install 20:47:59, process started 20:57:56 — the operator's live checks are
unblocked; the running binary contains every landed slice).
**Read first:** the worktree-root `.rules` (required), then
`kask/docs/plans/media-panel-transcript-bundle-plan-2026-11-04.md` (§4 carries
each slice's touchpoints, acceptance criteria, and risks; §5 verification
discipline), `kask/docs/research/media-transcript-interaction-study-2026-11-04.md`
(the research grounding), `kask/docs/diataxis/media_panel/reference.md` (the
landed panel contract, v3.3.0), the goal record
(`kanban_goal_get 70f687af-2783-4e3c-b4a0-a8571c2448f1`, spec frozen under
digest 249026534c7aa278967db4d05b097d83…), and the board "Media panel
transcript bundle" (`8820ae83-a57f-45d6-9caf-23320e687ca8`, Done column
verification-gated).

---

## Mission

Complete the four remaining slices of the operator-approved media panel
transcript-bundle plan. Goal text (frozen): "Refining and completing the
design and build of the media panel — focusing on the educt transcript
bundle: the synced recording from transcript through reel, per the
operator-approved implementation plan."

The four frozen criteria (verbatim; judge work ONLY against these — never
your own verdict on it):

1. "Closing the transcript sub-surface renders zero overlap at any pane
   height, pinned by a layout test over the close transition." — MET (S1).
2. "The transcript surface supports the reference interaction set:
   drag-select with a selection popover and keyboard shortcuts, in-transcript
   search with jump-to-match, playback speed on both media kinds, and layered
   in-panel correction through the existing correction tools." — HALF MET
   (S2 + S3 landed; S4 speed and S5 correction open).
3. "The reel composer edits order, inclusion, and in-reel strikes, previews
   the composed order, then composes and renders through the existing EDL
   path." — OPEN (S6).
4. "Highlights export as W3C Web Annotation JSON (position and quote
   selectors) and composed reels serialize to OpenTimelineIO." — OPEN (S7).

Judge after each slice (`kanban_goal_judge`, verdict `continue` expected until
all criteria are met); algedonic review and `kanban_goal_close` are the
operator's at full completion — closure is a HUMAN decision, never auto-close.

## Verified ground truth (checked against the tree, 2026-11-05)

**Repo:** `/home/mdz-axolotl/Clones/zed-kask` — a SHARED tree, other streams
may be editing concurrently. Pathspec commits only; check
`git diff --cached --stat` before any commit; verify the commit numstat
against your measured pre-commit diff; never a working-tree-wide mutation; one
repository, never a second clone.

**Landed (do not redo; all on origin/main unless noted):**
- S1 close-transition correctness: `828b9c09f7` + `f0a3cbd569` — the 240px
  usable-height floor moved to the content-flow embedder (`block_renderer` in
  hkask-viz-core, suppressed on load errors via `error_message`), workspace
  content container clips as backstop, `close_transcript_workspace` tears down
  `selection_mode`. Pin: `closing_transcript_leaves_widget_inside_pane`.
  Operator visually confirmed.
- S2 selection gestures + popover: core `df72da1487` (press-and-release
  drag-select across words via the pure `WordDragOutcome` machine; same-word
  release stays a plain click → click-to-seek preserved), remainder
  `8773678abd` (popover pinned above the list: Highlight / Cut / Copy time /
  Include in reel; Enter/Delete/Escape bound on the tray itself while it holds
  focus, so the label input keeps Delete as forward-delete text editing;
  teardown symmetry across close/open/surface-switch/reload).
- S3 in-transcript search: core swept into `b01e4a168b` (attribution
  corrected in `3d9c8ce409`; the plan's S3 section carries the traceable
  note), tail + docs `a23af25a8d`. Header input on the words surface; 150 ms
  debounce via `cx.background_executor().timer`; ASCII case-insensitive /
  non-ASCII exact matching per `educt_search_transcripts` (panel and server
  agree on what a match is); Enter/Shift-Enter cycle with follow-reveal
  jumps; match marks compose field-wise over highlight and strike overlays;
  honest count at every state; Escape clears; teardown symmetry with the
  popover.
- Post-S3 corrections on origin/main: `883971c07d` (the S3 focus test's false
  claim corrected — focus registration is render-time in the test harness;
  `activate_window` + notify are required to see a focus dispatch land),
  `a8b8a136b2` (shared job fixtures consolidated into
  `crates/media_panel/src/test_fixtures.rs`; layout tests now use the
  repo-local `crates/media_panel/test_data/viewer-layout.mp4` — the external
  vonnegut fixture dependency and its silent skip are GONE; a missing
  fixture fails loudly, never passes vacuously).
- Gates at handoff: `cargo test -p media_panel` 121/121 on the clean tree.
  CI: the census landing `a8b8a136b2` is green (run `42147369034`). The FR-4
  ratchet red was adjudicated 2026-11-05/06: nine FR clusters deduped in
  `532d8b6442` + `c10f39bb1d`, zero baseline change. The ratchet is red
  AGAIN (5613 vs the 5599 baseline at the 2026-11-06 scan) and this arc's
  OWN S5/S7 legs are the largest live debtor (+13 clusters): every arc's
  committed duplication is its own debt — dedupable scaffolding
  consolidates, and additions that survive as accepted duplication bump
  `kask/scripts/duplication-ratchet-baseline.txt` in the SAME commit with
  the justification (the gate's doctrine). The gate has a ±2-4 same-tree
  noise floor (calibration task `30796b8c` on the Ongoing Work board) —
  re-run before believing a ±1 breach.

**Goal/board state:** judge recorded `continue` 2026-11-05 with per-criterion
evidence. Board: S1–S3 done, S4–S7 backlog. The S4 card carries the
pre-verification comment (`f7b229c7` on task `ea1aa377`) — read it first.

## The work queue (dependency order; task ids on board 8820ae83)

1. **S4 playback speed 0.5×–2×** — task `ea1aa377-9f52-4549-abc4-604159cd0f3d`.
   The plan's gating risk is RESOLVED (full findings in the S4 card comment):
   the pinned rodio rev exposes `Player::set_speed(f32)` and the widget holds
   exactly that `Player` on both paths — audio-only and the video audio-master
   clock, so speed on the rodio player advances the consumed-audio clock that
   already paces video. The plan's pure rate function is needed for the
   wall-clock fallback (video without audio) and satisfies the acceptance pin
   (1.5× → 2/3 interval). Pitch shifts with speed (resampling, not
   time-stretch) — document honestly in reference.md at slice time.
   COORDINATION: the standing directive "hand off write scope with the other
   stream before touching `hkask-media-widget/src/{media_widget,transport}.rs`"
   — their widget work landed (`b38e97ab5c`), the crate is clean in-tree, they
   are now on FR-4 kask_bridge territory; get the operator's one-line
   confirmation, then claim, kata-record, red→green.
2. **S5 in-panel layered correction** — task
   `45c383ac-53f8-4679-9b4b-a1576946c5c4`. Rides S2's popover (S2 dependency):
   select misheard words → replacement field → targeted correction layer via
   `educt_store_layer` (kind=correction), then `educt_realign_transcript` on
   cardinality change. Read the server's correction-layer contract FIRST and
   pin the payload schema with a fixture. Single edit admission
   (`begin_edit` rejects overlap). Working-view selections MUST translate
   through the reanchored index map (the `.rules` retranscribe trap).
3. **S6 reel composer** — task `5e871bd4-a120-489b-a7dd-e1d554d9abea`
   (size L): ordered ClipSpec list; explicit order through
   `union_word_ranges` → `compose_reel_params` → `compose_workspace_reel`;
   in-reel strike = per-clip difference math; preview = auto-seek chain on
   the shared player; `pane_reel_compose` provenance must never parse as a
   researcher strike (extend the identity pin to the ordered form). Replaces
   the unordered composition — delete the unordered call sites in the same
   change. Titles are a declared stretch item, not in acceptance.
4. **S7 standards exports** — task `e0c1cf12-93e4-4ea7-8184-ad506c022559`:
   server-side (`kask/mcp-servers/hkask-mcp-media`): WebVTT with word-level
   karaoke cues; Web Annotation JSON (TextPositionSelector +
   TextQuoteSelector, motivation highlighting/tagging, creator
   researcher_supplied); OTIO serialization of composed reel EDLs. Pure
   deterministic serializers, fixture-pinned byte-for-byte, NO Python
   anywhere. Panel affordances in the highlights/reel sections + the
   inspector's Reel tab.

## Binding process directives

- Red→green for every behavior change: failing pin FIRST, run red, fix, run
  green. The oracle that adjudicated a claim re-runs against the change
  claiming it — no inherited green.
- Gates per slice: `cargo test -p media_panel` (121 passing at handoff, +N
  per new pin) plus `./script/clippy -p <every touched crate> --deny
  warnings`. A plain `cargo check` or green tests alone is NOT a clean claim.
  Live-mutation probes run with `--test-threads=1`.
- Shared tree: staged-index check before any commit; pathspec commits
  (`git commit -m "..." -- <your files>`); numstat verification against the
  measured pre-commit diff; NEVER working-tree-wide mutations (no bare
  `git stash`/`restore`/`checkout`/`reset --hard`). Other streams' uncommitted
  files are untouchable; their unpushed commits are never pushed or touched.
- Kanban: one state-mutating call per task per batch (moves AND comments
  race); kata record on the active task; Done is reachable only through
  `kanban_task_verify` with evidence.
- edit_file discipline: `old_text` verbatim from a FRESH read; re-read the
  landed bytes after edits (the tool can land a different byte arrangement
  than `new_text`); never chain byte-assumed anchors across edits on the
  same file.
- GPUI traps: timers via `cx.background_executor().timer` (NEVER
  `smol::Timer`); no `block_on` on the foreground; `AsyncApp` is not `Send`;
  `min_h_0`/`min_w_0` on shrinking containers; absolute children only under
  an explicit `relative()` anchor.
- TABLED by the operator — do NOT re-propose: the `.rules` addition for the
  shared-entity floor / embedder-owned floors (memory
  `media_panel:rules_proposal_status`; study §7; the fix itself landed in S1).
- Goal-intake session identity for any new records: curator thread
  `8314713b-0963-4787-8b4a-ba64689562aa` (never invent a source locator).

## Known traps specific to this code

- `InteractiveText` (crates/gpui/src/elements/text.rs) fires `on_click` only
  when mouse-down AND mouse-up land in the same range — cross-word
  press/release is click-free by construction (the drag pattern is banked in
  memory `media_panel:drag_selection_pattern`).
- `open_transcript` / `close_transcript_workspace` teardown must stay
  symmetric (media_viewer.rs) — the search state and the selection popover
  both reset through it.
- Layout floors belong to the embedding site, never the shared per-asset
  widget entity (memory `media_panel:layout_floor_discipline`).
- CI fails fast: while the `check` job fails, `kask-tests` is cancelled —
  adjudicate whose failure before reacting
  (`gh run view <id> --repo mdz-axo/zed-kask --log-failed`).
- `audio_pumps_ahead_of_the_playback_clock` is already hardened (cumulative
  `audio_appended()` assertion) — do not re-harden it.
- Layout tests run on the repo-local
  `crates/media_panel/test_data/viewer-layout.mp4` — no silent skip; a
  missing fixture fails loudly.
- The GPUI test harness runs each test on its own thread and only
  effect-driven draws run — focus tests need `activate_window` plus a notify
  to see the dispatch land (reference model v3.3.0 records both findings).

## End-to-end test asset

The Dunning recording: gallery asset `dunning-knowing-yourself-2017.mp4`
(id `80f804a6-f5da-4119-bcfc-292403b3dd61`), transcript
`5509bb94-6a9e-41b3-afb9-512e4db618e1` (14,270 words, `re_anchored`,
~82.7 min).

## First actions at pickup

1. `kanban_goal_get 70f687af-2783-4e3c-b4a0-a8571c2448f1` + plan §4 for the
   next slice; read the S4 card's pre-verification comment.
2. `git status --short` / `git log --oneline -5` /
   `gh run list --repo mdz-axo/zed-kask --workflow "Kask invariants"
   --branch main --limit 3` — the tree moves; re-verify line anchors
   (file:line cites in the plan and in this prompt may have drifted).
3. Ask the operator two things: (a) the results of their live checks on the
   rebuilt binary — S2 remainder (drag across Dunning words → the popover
   tray: Delete cuts, Enter names, same-word click still seeks) and S3 (the
   search input: type a word → count beside it, Enter/Shift-Enter cycle,
   Escape clears, a wordless view states why it cannot search) — the binary
   at handoff contains everything landed; (b) the one-line S4 write-scope
   confirmation for `hkask-media-widget`.
4. Claim S4, kata-record it, red→green; report per slice with commit hashes,
   gates run, and net lines.
