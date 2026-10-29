---
title: "Media system repair plan — refactor-architecture run"
audience: [architects, developers, operators]
last_updated: 2026-10-28
version: "1.0.0"
status: "Proposed — repair plan delivered for operator review; no production code changed in the commissioning session"
domain: "Media / transcript capabilities"
mds_categories: [domain, lifecycle]
---

# Media system repair plan — refactor-architecture run (2026-10-28)

**Commission:** operator commission, this session — produce the prioritized,
evidence-backed repair plan returning the transcript-linked media system to
the reference model. No production code changes were made in this session;
the plan is the deliverable.

**Method:** the `refactor-architecture` skill, every stage template rendered
with full inline context (target surface, reference model verbatim, seed
findings at file:line, scope bounds): ra-explore → RA-1 → ra-deepen →
ra-audit → ra-strangle → ra-verify.

- RA-1 (`lisp_eval`, decision `proceed_to_refactor` — the commission is the
  operator's explicit decision signal) → routed to `ra-audit`.
- RA-2 (`lisp_eval`, `target_condition_status: unmeasured`, code-level checks
  `unmeasured` because nothing was changed to check) → **`partial`** — the
  honest verdict for a plan-only session. The implementing session re-runs
  ra-verify per domain after each repair lands.

**Tree state:** every citation below re-resolved at HEAD `161ccfe035`
(2026-10-28) during this session's bounded evidence pass. Code citations are
`file:line` at that tree; doc citations are section-level (docs re-anchor by
name, not line, to avoid citation rot).

**Live state (honest limitation):** the running media server currently has no
organized gallery (`gallery_status` → `no_gallery`) and zero stored transcripts
(`educt_list_transcripts` → `count: 0`). The operator's live-observed failures
(from prior sessions, named in the seed findings) could not be re-observed
against live rows; seed 2 verifies **structurally** (code + docs + history),
not against a live transcript row.

## Reference model (verbatim — never paraphrased)

Source: `~/Documents/zk-data/zk-reference-open/reference-docs/reduct-transcript-model.md`
(zk-ref-open; CC BY 4.0; stands in for reduct-help vendor docs):

> Transcript-linked media couples a time-aligned transcript with its source
> recording so that text and media address each other. The reference model:
> (1) the transcript is the index — every word (or doc-range) carries a time
> span, so any text selection maps to media time and any media span maps to
> text; (2) a reel is an ordered composition of doc-range clips — the reel
> references transcript spans, never re-encodes media, so a reel is cheap,
> reversible, and always in sync with the transcript; (3) recordings are
> project-scoped assets that media (video/audio) uploads attach to, and
> transcription status is a per-recording state; (4) highlights are labeled
> selections — the semantic layer that reels are composed from. The
> discipline the model teaches: edit at the reference layer (spans, orders,
> labels), render only on demand — never mutate media to edit meaning.

Marker convention used throughout: **[VERIFIED]** cites evidence that
re-resolves at the cited line at HEAD `161ccfe035`; **[PROPOSED — not run]**
marks a repair or acceptance criterion authored this session and not executed.

---

## 1. Conformance map

### Capability 1 — the transcript is the index (text↔media mapping, both directions)

**Present and conformant:**

- Word-level timings flow end to end: the server stores word-timed
  `TranscriptBundle`s; the panel strict-parses them into
  `TranscriptPaneState` with `PaneWord` timings.
  **[VERIFIED `crates/media_panel/src/media_viewer.rs:148-165`]**
- Text→media: every word is a click range that seeks the bound player to its
  start (S3b), over `MediaWidget::seek_to_secs`.
  **[VERIFIED `media_viewer.rs:4015-4019`; `crates/hkask-media-widget/src/media_widget.rs:1095-1099`]**
- Media→text: a 250ms (4 Hz) GPUI-native follow task polls the one position
  accessor (`transport_position_secs`), maps position→containing word
  (`current_word_index`), and renders the mark per block; polling is gated on
  playing. **[VERIFIED `media_viewer.rs:41-45`, `:2842-2889`, `:5496-5498`,
  `:4068-4074`; `media_widget.rs:1062-1085`]**

**Absent — the operator-facing violation (seed finding 1, re-verified):**

- Nothing scrolls the transcript to keep the marked word visible. The
  transcript container is a plain `overflow_y_scroll` div and the blocks are
  non-virtualized `sections.child` elements; there are **zero**
  `scroll_to`/`scroll_to_item` calls anywhere in `crates/media_panel/**`
  (grep-verified). On a long recording the mark is off-screen within seconds
  of playback — the mapping exists in data but is unusable in experience.
  **[VERIFIED `media_viewer.rs:4407-4411` (container), `:4033-4065`
  (non-virtualized blocks), grep sweep (zero scroll calls)]**
- The docs pin the drifted contract: the diataxis reference defines sync as
  click-seek plus "a 4 Hz follow-along marks the containing word" — mark-only,
  no viewport tracking. **[VERIFIED
  `kask/docs/diataxis/media_panel/reference.md` § Transcript surface, Sync
  bullet]**

**Repair:** R1 (§3). In-tree precedent exists: the project panel's
`scroll_handle.scroll_to_item_strict(index, ScrollStrategy::Top/Center)`
**[VERIFIED `crates/project_panel/src/project_panel.rs:3223-3245`]**.

### Capability 2 — a reel is an ordered composition of doc-range clips (never re-encodes)

**Conformant; no repair needed** (pin-only):

- The workspace composes reels at the reference layer: Keep ops from
  researcher highlights, Cut ops from strikes, per mode (Assemble / Reduce /
  Mixed), stored via `educt_store_layer` and rendered on demand via
  `educt_render_edl` — a two-invoke chain whose render lands the reel as a
  playable asset. **[VERIFIED `media_viewer.rs:2276-2375` (`compose_workspace_reel`),
  `:4371-4381` (mode affordances), `:404-412` + `:1780-1790` (reel selection
  keys on provenance, never label text)]**
- Reel membership is rebuilt from researcher-authored selections on load;
  renders and durable exports survive transcript deletion, detached from the
  removed record. **[VERIFIED `media_viewer.rs:1780-1790`; commit `785ea7f947`
  delete semantics; server detach model in
  `kask/docs/reference/mcp-servers/media.md` § Transcript-as-timeline]**
- No media is mutated to edit meaning anywhere in the compose/render path
  (EDL word ranges → ffmpeg clip/concat at render time only).
  **[VERIFIED `media_viewer.rs:2359-2375`; server render semantics per
  `media.md` § Transcript-as-timeline]**

### Capability 3 — recordings are project-scoped assets; transcription status is per-recording

**Present:**

- Gallery assets exist as project-scoped recordings (video/audio), addressed
  by stable `gallery_asset_id`. **[VERIFIED
  `crates/media_panel/src/media_viewer.rs:309-313` (`MediaAsset.gallery_asset_id`)]**
- The transcript store carries the recording relationship: live
  `gallery_asset_id` plus immutable `source_gallery_asset_id`, with a detach
  trigger on asset deletion. **[VERIFIED
  `kask/mcp-servers/hkask-mcp-media/src/transcript_store.rs:36-46`, `:104-113`]**
- Recordings delete: the two-click Delete (arm → Confirm delete) on the
  Transcript tab's recordings rows dispatches `educt_delete_transcript`,
  disarms on dispatch and on listing reloads, is a sibling of the open area
  (opening and deleting never fire together), and is pinned by
  `transcript_browser_delete_confirms_and_closes_the_edit`. **[VERIFIED
  `media_viewer.rs:4533-4540` (arm/confirm), `:1815-1859` (dispatch),
  `:1955-1967` (disarm-on-reload), `:9663` (pin test); commit `785ea7f947`]*

**Absent (seed findings 2 and 3, re-verified):**

- **Per-recording transcription status does not exist.** The Transcript tab's
  "Recordings" rows are transcript records, not recordings: a recording that
  has not been transcribed is invisible in the tab, and rows show
  duration · words · alignment — no transcription state. The only per-asset
  signal is the Detail tab's linked-transcripts list. **[VERIFIED
  `media_viewer.rs:4450-4494` (row fields), `:4436-4442` (empty state),
  `:1224-1255` (Detail linked transcripts)]**
- **The panel has no create-from-media path.** There are zero
  `transcribe`/`job_submit`/`gallery_organize` dispatch sites in the panel;
  creation is chat-agent-only, per the recorded viewing-surface ruling.
  **[VERIFIED grep sweep; `crates/media_panel/src/media_panel.rs:88-89` (the
  ruling itself)]**
- **The workspace is audio-keyed (seed finding 2 — verified structurally):**
  - The transcript's media identity is `bundle.audio_path`, documented "Path
    to the audio file (WAV, 16kHz mono)" — audio-named even when a video
    could be the source. **[VERIFIED
    `kask/mcp-servers/hkask-mcp-media/src/transcript.rs:46-56`]**
  - The store keys `media_path = bundle.audio_path`; the workspace player and
    the recording's display name are then derived from that path — the video
    gallery asset and its transcript are only weakly joined (an optional asset
    link the workspace never reads for playback). **[VERIFIED
    `transcript_store.rs:285-299`; `media_viewer.rs:3845-3874`]**
  - The workspace rebuilds its widget body by path-extension inference and
    renders a fixed 140px player strip — an audio-shaped workspace even when
    the transcript's source was a video. **[VERIFIED
    `media_viewer.rs:5565-5580` (`transcript_media_body`), `:3899-3907`
    (140px strip)]**
  - `recording_display_name` strips 32/64-hex content-addressed stems to
    "Untitled recording" — fossil evidence that derived, hash-named files
    became the operator-visible recording identity. **[VERIFIED
    `media_viewer.rs:4912-4924`]**
  - The docs say "Transcribe audio" while the tools accept audio or video.
    **[VERIFIED `kask/docs/reference/mcp-servers/media.md` § Audio capture and
    transcription tool table]**
  - **The conformant mechanism already exists server-side:** the async job
    path keys the bundle to the ORIGINAL input path (video accepted) and
    deletes the temporary STT extraction; the panel's kind inference handles
    `.mp4` → video (pinned). The drift is in what transcripts were keyed to,
    what the panel derives from the result, and what the docs claim — not in
    a missing server capability. **[VERIFIED
    `kask/mcp-servers/hkask-mcp-media/src/tools/jobs.rs:191-224`;
    `media_viewer.rs:9029-9033` (kind-inference test)]**

**Repairs:** R2 (recording-first rows with status), R3 (recording-keyed
workspace), R5 (the operator's product question on the create affordance).

### Capability 4 — highlights are labeled selections

**Conformant; no repair needed** (pin-only):

- Highlight create/rename/delete over word ranges with the label editor;
  select mode toggles word clicks between seek and range-building; strikes
  render line-through; direct selection is gated per alignment state with the
  gate surfaced on the row before the click. **[VERIFIED
  `media_viewer.rs:2581-2585` (label editor), `:3950-3952` (select mode),
  `:4090-4110` (overlays + strikes), `:5184-5189` (alignment gate),
  `:4485-4494` (gate note on the row)]**

---

## 2. Findings, ranked

Live-observed operator-facing failures first (per the commission); each names
its repair.

| # | Finding | Evidence | Repair |
|---|---------|----------|--------|
| F1 | **Follow-along does not track along.** The 4 Hz mark exists; nothing keeps it visible; capability 1 is violated in experience on any long recording. The capability shipped "green" because the test surface ends at the mechanism (`current_word_index_marks_the_containing_word`) and the docs define mark-only as the contract. | [VERIFIED `media_viewer.rs:41-45`, `:2842-2889`, `:4068-4074`, `:4407-4411`, `:4033-4065`; grep: zero scroll calls; `:9056`; diataxis reference Sync bullet] | R1 |
| F2 | **The recording model is split and status-less.** Recordings are gallery assets in Library (no transcription state) and transcript records in the Transcript tab (recordings without transcripts invisible); no per-recording transcription status anywhere; no panel create-from-media path. Capability 3's "transcription status is a per-recording state" is unimplemented. | [VERIFIED `media_viewer.rs:4450-4494`, `:4436-4442`, `:1224-1255`; grep: zero create dispatch sites; `media_panel.rs:88-89`] | R2, R5 |
| F3 | **The workspace is audio-keyed, not recording-keyed.** A video recording's transcript keys to an audio path; the workspace plays a 140px audio strip and names the recording from a file stem (with hash-stripping for content-addressed names). The video+audio+transcript bundle of the reference model never assembles in the workspace. Verified structurally; live store empty at session time. | [VERIFIED `transcript.rs:46-56`, `transcript_store.rs:285-299`, `media_viewer.rs:3845-3874`, `:3899-3907`, `:5565-5580`, `:4912-4924`; `media.md` "Transcribe audio"; conformant mechanism `jobs.rs:191-224`] | R3 |
| F4 | **Dead-end error states — mostly closed; two residuals.** The decode-failure trap class was fixed and pinned (dismissible status, Back above player, selection reachable). Residuals: (a) a failed recordings listing renders a full-tab empty state whose only recovery is a tab round-trip (no in-place retry); (b) job history is process-local (documented, by design) — an in-flight transcription status cannot survive a server restart, which R2 must degrade honestly. Zero production `unwrap()` across all 11 panel+widget files. | [VERIFIED commit `00a365a190`; `media_viewer.rs:9598` (pin), `:3876-3891` (Back), `:3908-3915` (status), `:546-552` (tab-round-trip recovery), `:4420-4434` (empty state); unwrap sweep] | R2 (honest in-flight status) |
| F5 | **The suite-green traps that shipped the drift.** Docs pin mark-only sync and audio-only transcription as the contracts; the follow-along acceptance ended at the mechanism; fixtures are audio-path shaped. These are findings, not evidence of health — see §5. | [VERIFIED diataxis Sync bullet; `media.md` tool table; `:9056`; audio-path fixtures throughout the layout tests] | rewritten with R1/R3 |
| F6 | **Monolith friction (enabling, not operator-facing).** `media_viewer.rs` is 9,772 lines carrying five tabs' state in one struct; every repair above pays a navigation tax. The workspace module extraction rides R1 rather than shipping as a standalone refactor. | [VERIFIED `wc -l` + struct fields `media_viewer.rs:358-419`] | rides R1 |
| F7 | **Hygiene minors.** A stray file named with an ffmpeg select-filter expression sits in `kask/mcp-servers/hkask-mcp-media/` (observed via `ls`; shell-misadventure residue). The full media suite has a known unrelated failing test (`range_extraction_gates_on_start_and_keeps_source_pts`, `video/ffmpeg.rs:1137`) per the speaker-identity board's live checkpoint — the implementing session must not blame its change for it. | [VERIFIED `ls`; board comment, educt speaker identity board] | sweep with R1's commit |

## 3. Strangler-fig sequence

One domain per commit; each repair names what it replaces (old path deleted,
not orphaned), an implementation-pass acceptance criterion a later session can
run, and the reference-layer discipline check. Order: R1 first (highest-frequency
operator pain, no product question, unblocks the workspace module), then R2
(the recording model R3 rides on), then R3, with R4 riding R1's module
extraction, and R5 an operator decision before any panel create affordance.

### R1 — Follow-along viewport tracking (capability 1)

**Replaces:** the non-virtualized block rendering (`sections.child` per block)
and the mark-only follow contract — the diataxis Sync bullet is rewritten in
the same change to state the visible-tracking contract. The workspace state
(follow task, mark, selection, reel selection) extracts into a
`transcript_workspace` module inside `crates/media_panel/` (fork-side, no new
crate, no upstream surface).

**Implementation-pass acceptance criteria** (later session runs; each is RED
on the current tree):
1. With a fixture transcript long enough to overflow the viewport, playback
   advancing the mark beyond the initial viewport → assert the workspace
   scroll offset contains the marked block (virtualized list + scroll handle;
   precedent `project_panel.rs:3223-3245`).
2. A manual scroll during playback suspends auto-follow (mark keeps updating,
   viewport does not jump); the follow toggle re-enables tracking and scrolls
   to the current mark.
3. Existing pins stay green: `current_word_index_marks_the_containing_word`,
   `word_selection_state_machine`, `transcript_pane_wraps_within_narrow_dock`,
   `transcript_media_failure_keeps_navigation_and_curation_reachable`.
4. `cargo test -p media_panel` and scoped `./script/clippy` pass; the diataxis
   reference Sync bullet re-anchored in the same commit.
5. Measure before/after render cost on a long fixture (the virtualization win
   is a hypothesis, not a premise — record the numbers in the commit).

**Discipline check:** edits no media and no transcript content; reads word
timings (reference layer) and scrolls text (presentation layer); renders on
demand. Conforming.

### R2 — Recording-first recordings rows + per-recording transcription status (capability 3)

**Replaces:** the transcript-records-only browser rows — the Transcript tab
lists gallery recordings (video and audio assets), each with a transcription
status and its attached transcripts beneath it. The unfiltered
all-transcripts row surface is deleted; the `educt_list_transcripts` call
remains, joined per recording.

**Implementation-pass acceptance criteria:**
1. A recording with no transcript renders status "not transcribed" and is
   visible in the tab (today it is invisible); a recording with a transcript
   renders status + opens the workspace on click.
2. In-flight status degrades honestly: a linked running `transcribe` job
   (process-local) shows in-flight, and after a server restart the row shows
   the durable truth (no fabricated in-flight state).
3. The two-click delete and its pin
   (`transcript_browser_delete_confirms_and_closes_the_edit`) survive on the
   recording's attached-transcript rows.
4. The strict row parser (`transcript_listing_is_strict`,
   `TranscriptSummaryRow`) is updated in the same change — the strict-parse
   seam means new fields must land with the parser or rows are silently
   dropped.
5. `cargo test -p media_panel`; docs (diataxis media panel reference,
   `media.md` recordings wording) re-anchored in the same commit.

**Discipline check:** status is a reference-layer read (asset + transcript
link + job state), rendered on demand; no media mutation. Conforming.

### R3 — Recording-keyed workspace (video-first) (capabilities 3 + 1)

**Replaces and deletes:**
- `transcript_media_body` path-extension inference (`media_viewer.rs:5565-5580`)
  — the workspace receives the recording (asset id, kind, playable source)
  and builds the widget body like the Media tab does; the inference helper is
  deleted, its test re-anchored to the recording-keyed path.
- The hash-stem branch of `recording_display_name` (`media_viewer.rs:4912-4924`)
  — display names flow from the gallery asset; the plain-stem fallback stays
  for path-backed, unindexed recordings.
- The fixed 140px player for video (`media_viewer.rs:3899-3907`) — kind-aware
  player area: video scales per the Media-tab precedent
  (`viewer_video_area_scales_with_window_size`), audio keeps the bounded strip
  (the `00a365a190` recovery contract is preserved for audio).

**Implementation-pass acceptance criteria:**
1. A video-keyed transcript renders the video player in the workspace
   (kind-aware area), with click-to-seek and follow-along intact.
2. An audio-keyed transcript is unchanged: the failure-recovery pin
   (`transcript_media_failure_keeps_navigation_and_curation_reachable`) and
   the 140px audio strip stay green.
3. Server-side: no handler change is required for keying (the store already
   carries the asset link, and the job path already keys the bundle to the
   original input — `jobs.rs:191-224`); the change is panel consumption plus
   docs: `media.md` transcription rows say audio **or video**, and state the
   recording-keyed creation flow (transcribe the recording's source; store
   against the recording's `gallery_asset_id`).
4. `cargo test -p media_panel -p hkask-media-widget`; scoped clippy; docs
   re-anchored in the same commit.

**Discipline check:** playback identity is a render-time read of the recording
asset; no media is copied or mutated; STT provenance (`media_path`) remains
stored for lineage. Conforming.

### R4 — Confirm-arm unification (rides R1's module extraction)

**Replaces:** the two two-click confirm state machines
(`confirm_delete: Option<usize>` for Library rows,
`confirm_delete_transcript: Option<String>` for recordings rows) and their
duplicated disarm-on-reload logic, unified as one keyed confirm-arm state.

**Implementation-pass acceptance criteria:** existing delete pins stay green
(`transcript_browser_delete_confirms_and_closes_the_edit`, the gallery delete
tests); a new property test: arming then a listing reload disarms (both row
families).

**Discipline check:** presentation-layer interaction state only. Conforming.

### R5 — Operator product question (functional decision — the user's, not presumed)

Capability 3's "create-from-media" is chat-driven today by recorded ruling
(`media_panel.rs:88-89`). The functional question, presented not presumed:

- **Option A — the panel gains a Transcribe affordance on recording rows**
  (dispatching the existing `job_submit transcribe` against the recording's
  source, so the operator never depends on an agent session). Functional
  consequence: the operator can take a recording from import to transcript
  to reel entirely in the panel; revises the viewing-surface ruling by
  operator choice.
- **Option B — creation stays chat-driven**; R2's status surface makes the
  in-flight and missing states visible so the operator knows what to ask
  for. Functional consequence: the panel remains read/curation-only; the
  operator asks the chat agent to transcribe.

No work is sequenced under R5 until the operator answers.

---

## 4. Verified / proposed markers

- **[VERIFIED]** items: the conformance-map claims (present and absent, all
  four capabilities), the seven ranked findings' evidence chains, the
  duplication audit inputs, and the stage receipts — **35 cited evidence
  points**, each re-resolved at the cited line at HEAD `161ccfe035` this
  session (conformance map: 24; findings: 11 additional, overlapping the
  map's citations).
- **[PROPOSED — not run]** items: R1–R4 with their acceptance criteria, the
  doc rewrites riding them, the confirm-arm unification, the R5 question,
  and the virtualization render-cost measurement — **11 authored, none
  executed**. No proposed check is presented as executed anywhere in this
  plan.

## 5. Tests that pin drift (named)

These are findings, not evidence of health — each must be rewritten or
re-anchored in the same change as its repair:

1. **The diataxis Sync bullet** (`kask/docs/diataxis/media_panel/reference.md`
   § Transcript surface) — pins mark-only follow-along as the contract; agents
   read docs as ground truth. Rewritten by R1.
2. **`media.md` transcription rows** (§ Audio capture and transcription) —
   "Transcribe audio" pins audio-only creation while the tools accept
   audio/video. Rewritten by R3.
3. **The S3c acceptance surface** — `current_word_index_marks_the_containing_word`
   (`media_viewer.rs:9056`) pins the mechanism and nothing pins the
   capability; the follow-along card closed green on mark-only. The trap
   class: acceptance at the mechanism level. Closed by R1's visibility test.
4. **Audio-path fixtures throughout the workspace/layout tests** (e.g. the
   failure-recovery pin's fixture) — they encode the audio-keyed shape as the
   expected world; under R3 they re-key to recording-keyed fixtures. Not
   wrong today; re-anchored by R3.
5. **`transcript_media_body_infers_kind_and_parses`** (`media_viewer.rs:9029`)
   — currently pins path-extension inference including video; its job moves
   under R3 to the recording keying and the test re-anchors. Not a bad test —
   a re-anchor target.
6. **`transcript_listing_is_strict`** (`media_viewer.rs:7980`, row struct
   `:61-70`) — strict wire parse; R2's new row fields must land with the
   parser update in the same change or new rows are silently dropped (the
   known composition-breaks-at-the-seam trap).

## 6. Blockers, limitations, and shared-tree sequencing

- **No blocker was surfaced** that stops this plan: the discovery pass stayed
  within one bounded pass over the three crates; every stage template carried
  full context (the prior run's failure mode — context-less template calls —
  was avoided; one template call was rejected for a missing `glossary_terms`
  field and resent complete).
- **Honest limitations:** (a) live stores are empty (no gallery organized,
  zero transcripts), so seed 2 verified structurally, not against a live
  row; (b) R5 is an open functional question for the operator.
- **Shared-tree sequencing for the implementing session:** active media boards
  are **Media layers v1** (`7ddf45f5`), **educt speaker identity**
  (`0e97bd22` — one in-progress task: Phase 3 evaluation/lifecycle gates for
  voice identification, disjoint from R1–R3 surfaces), and **Transcript bench**
  (`7c94792b` — nothing in progress). Check each board's In Progress column
  before starting; pathspec commits only; the known unrelated failing media
  test (`range_extraction_gates_on_start_and_keeps_source_pts`,
  `video/ffmpeg.rs:1137`) and the intermittently-failing
  `educt_render_edl_produces_a_clip_from_a_real_media_file` are pre-existing —
  scope test runs to the touched crate and re-run the flaky ones in isolation
  before blaming a change.

## 7. Change log

- This plan document added; committed with a pathspec commit limited to this
  file. No code, test, or doc-behavior changes were made in this session.