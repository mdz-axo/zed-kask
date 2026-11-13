---
title: "media panel — transcript-bundle implementation plan (from the 2026-11-04 interaction study)"
audience: [developers, architects, operators]
last_updated: 2026-11-06
version: "1.0.0"
status: "Active"
domain: "Media panel / UI"
mds_categories: [domain, composition, lifecycle]
---

# media panel — transcript-bundle implementation plan (from the 2026-11-04 interaction study)

title: Media panel transcript-bundle implementation plan
audience: operator (PI) + build agents
last_updated: 2026-11-06
status: proposed — awaiting operator confirmation; implementation deliberately separated
source_study: kask/docs/research/media-transcript-interaction-study-2026-11-04.md (research run ce75b8fc6a8392f5)
predecessor: kask/docs/plans/media-panel-redesign-plan-2026-11-02.md (landed phases A–D)

## 0. How this plan was composed (method, decisions made)

1. **Slices derive from the study's gap analysis**, not a fresh wish list: one
   vertical slice per user-visible capability, ordered P0 (defect) → P1 (parity) →
   P2 (composer, interchange). Nothing enters the plan without a study anchor.
2. **One slice = one vertical cut** — code, pin test, and doc update in the same
   change; every acceptance criterion names its oracle up front and is falsifiable.
3. **Slice independence** — no slice's validation depends on an unfinished sibling;
   each phase is shippable alone. Dependencies that do exist are declared (S5 rides
   S2's popover; S6 rides S2's selection model).
4. **Replaces/deletes stated per slice** (P5.5): net lines reported at each close.
5. **Proposal separated from implementation**: on confirmation the goal is
   registered (criteria frozen), slices become tasks, kata state rides the active
   task, phase gates run through goal judgment. Nothing below is started yet.
6. **Explicitly deferred**: P3 player affordances (fullscreen, pop-out,
   corner-resize), collaboration/commenting, title-card overlays (stretch item
   inside S6), multicam — out of the single-operator bench scope.

## 1. Scope

In: the transcript sub-surface and its extensions in `crates/media_panel`,
`crates/hkask-media-widget`, and export formats in `kask/mcp-servers/hkask-mcp-media`.
Out: P3 affordances, server STT/model changes, any upstream-Zed surface (all work
stays within existing fork-owned seams; any NEW zed-side file carries its
DIVERGENCE.md row in the same change).

## 2. Slice inventory

| Slice | Objective (user-visible) | Phase | Size | Goal criterion |
| --- | --- | --- | --- | --- |
| S1 | Close-transition correctness: zero overlap any pane height; teardown symmetry | A | M | C1 |
| S2 | Drag-select + selection popover + keyboard shortcuts | B | M | C2 |
| S3 | In-transcript search with jump-to-match | B | S | C3 |
| S4 | Playback speed 0.5×–2×, both media kinds | B | M | C4 |
| S5 | In-panel transcription correction (layered, gated) | B | M | C7 (proposed amendment) |
| S6 | Reel composer: order, inclusion, in-reel strike, preview | C | L | C5 |
| S7 | Standards exports: Web Annotation JSON, WebVTT, OTIO | C | M | C6 |

Proposed amendment for confirmation: the study's §8 criteria list has six; the plan
maps S1–S7 to seven criteria **C1–C7** by adding "in-panel transcript correction"
(C7↔S5). Veto restores the original six (S5 then rides as a non-criterion task).

## 3. Phases and gates

- **Phase A — correctness (S1).** Gate: new pin test green; existing
  `closing_the_transcript_returns_to_the_asset_workspace` stays green; `./script/clippy`
  over touched crates clean; operator re-tests the live repro (close transcript in a
  short pane).
- **Phase B — interaction parity (S2, S3, S4, S5).** S2 first (popover hosts S5's
  action; S3/S4 independent, any order). Gate: per-slice acceptance green; a
  `kanban_goal_judge` pass over C2–C4 and C7 with per-criterion evidence.
- **Phase C — composer and interchange (S6, S7).** S7 is server-side and can run
  parallel to S6. Gate: C5, C6 judged with evidence; rendered-reel round trip
  exercised on the Dunning recording end-to-end.
- **Phase D — closure.** Full goal judgment against frozen criteria → algedonic
  review → operator closure decision (human; `kanban_goal_close` never automatic).

## 4. Slice details

### S1 — Close-transition correctness (P0)

**Objective:** closing the transcript sub-surface never paints outside the viewer
pane, in any pane height; the close path's state teardown matches the open path's.

**Anchor:** study §1 (mechanism analysis); GPUI discipline rules 1–2 and 5 (study §7).

**Touchpoints:**
- `crates/media_panel/src/media_viewer.rs:5109-5122` — workspace content
  container (add `overflow_hidden` — done in S1, `828b9c09f7`, with the
  `media-viewer-content` test hook)
- `crates/media_panel/src/media_viewer.rs:2515` vs `:2443-2508` —
  `close_transcript_workspace` teardown symmetry (add `selection_mode = false`,
  list-state reset; the `selection_mode` half landed in S1, `828b9c09f7`)
- `crates/hkask-media-widget/src/media_widget.rs:1049-1063` — where the unconditional
  `min_h(240px)` floor lived (sub-item S1b: move floor ownership to embedders —
  done in S1, `828b9c09f7`; the site now documents the move)
- viewer tests module (`media_viewer.rs` `mod tests` :10278; layout-test
  precedents in `mod viewer_layout_tests` :15345) — the new pin test's home

**Design:**
- (a) Pin test FIRST (red before the fix): render the workspace in a short pane,
  open the transcript, close it, assert via `debug_bounds` that the `media-widget`
  bounds stay inside the viewer pane. This is also the falsifier for the study's
  mechanism claim — if the pin test is green while the operator still sees the
  overlap, the diagnosis redirects (GPU surface caching after rebuild).
- (b) `overflow_hidden()` on the workspace content container.
- (c) S1b (root tier): the 240px floor exists for indefinite-height inline flow;
  survey embedding sites (`hkask_viz_core` block renderer) and move the floor to the
  embedder wrappers so definite-height panes can shrink the widget. Land as its own
  commit inside S1 with the widget layout tests re-run.

**Acceptance criteria (oracle named):**
1. New layout test `closing_transcript_leaves_widget_inside_pane` — red before,
   green after (GPUI test harness, `debug_bounds`).
2. Existing close-path test stays green (no behavior regression).
3. New pure state test: after `close_transcript_workspace`, `selection_mode` is
   false (red today — the asymmetry is the pin).
4. `./script/clippy` over `media_panel` + `hkask-media-widget` clean.

**Replaces/deletes:** nothing structural; S1b replaces the unconditional widget
floor with embedder-owned floors.

**Risks:** clip degrades to visible truncation at very short panes (acceptable,
documented); S1b touches the inline conversation renderer — needs its layout tests
(`transcript_pane_wraps_within_narrow_dock` class) green; concurrent streams —
pathspec commits per `.rules`.

### S2 — Selection gestures, popover, shortcuts (P1)

**Objective:** drag across words selects a range; the selection opens a popover
(Highlight / Cut / Copy time / Include in reel); Enter confirms the label, Escape
cancels, Delete cuts; the two-click range stays as the fallback gesture.

**Anchor:** study §6 P1-2; Reduct's verified select-to-act pattern; Shneiderman
direct manipulation; MeetScript low-effort-dominance.

**Touchpoints:** `handle_word_selection` (`media_viewer.rs:3350`), word blocks
(`render_workspace_block:6987`), label editor (`open_label_editor:3979`,
`confirm_label_edit:4015`), strike path (`strike_selected_range:3566`,
`create_workspace_strike:3587`), gate (`highlight_range_gate:8676`).

**Landed:** S2 complete 2026-11-04 — the drag core in `df72da1487`
(WordDragOutcome machine, live anchor/target marks, both interaction modes),
the popover remainder in `8773678abd` (selection popover pinned above the
list: Highlight / Cut / Copy time / Include in reel; Enter/Delete/Escape on
the tray, never the label input where Delete is text editing; Include flips
membership over an existing highlight's exact range or creates with the
default label over a fresh range — the reload includes it; teardown symmetry
on close/open/surface-switch/reload). Pins: `word_drag_state_machine`,
`selection_popover_opens_for_drag_and_two_click_ranges`,
`selection_popover_keys_highlight_cut_and_dismiss`,
`popover_cut_dispatches_the_strike_path`,
`popover_include_toggles_reel_membership`,
`popover_copy_time_writes_the_span`, `selection_popover_resets_with_the_surface`.

**Design:** drag = pointer down on a word + move + up (threshold ~3px to stay
compatible with click-to-seek), range = anchor→last hovered word, union across block
boundaries (same union math as the two-click anchor). Popover renders pinned above
the list (the label-editor pin pattern, `media_viewer.rs:6533-6541`) and hosts
actions that reuse existing dispatchers — no new server calls.

**Acceptance:**
1. Interaction test: drag across three words selects exactly the union range
   (scripted; red today).
2. Popover appears for both drag and two-click ranges; action tests: Highlight
   opens the label editor; Delete strikes; Include toggles reel membership.
3. Click-to-seek still works below the drag threshold (existing seek tests stay
   green).

**Replaces/deletes:** the two-click range remains but becomes the fallback; no
deletion.

**Risks:** virtualized-list hit-testing across blocks (selection spans blocks —
union math already exists); don't break word-click seek (threshold).

### S3 — In-transcript search (P1)

**Objective:** a search input in the transcript surface header; live substring
matching over the loaded transcript; match highlighting; Enter/Shift-Enter cycle;
match count; jump scrolls the block into view.

**Anchor:** study §6 P1-3; boundary note — cross-recording search stays a
Steer/`educt_search_transcripts` concern, not this surface.

**Touchpoints:** header items (`render_workspace_header_item:6619+`), pane blocks
(`build_pane_blocks:9510`), reveal machinery (`follow_marked_block_into_view:4773`),
field-wise mark composition (`resolved_highlight_runs:8747` — match styling composes
with highlight/cut marks).

**Landed:** S3 complete 2026-11-04 — the core (search state, debounced rescan,
reveal jump, header input, match marks, seven pins) landed inside the
census stream's `b01e4a168b` ("Delete the test-only orphans the
three-family census surfaced", pushed to origin/main — swept by a `git add
-A` while the S3 work sat uncommitted mid-debug; the shared-tree hazard,
recorded here for attribution), the final tail in `a23af25a8d`. Pins:
`transcript_search_matches_orders_maps_and_edges`,
`transcript_search_status_counts_zero_and_degrades`,
`search_marks_compose_with_highlight_and_strike_overlays`,
`transcript_search_resets_with_the_surface`,
`transcript_search_finds_cycles_and_lands_the_current_match`,
`transcript_search_rescan_debounces_the_burst`,
`transcript_search_focus_suspends_follow_and_blur_restores`. The focus
registration note (corrected 2026-11-05 after the operator's challenge
forced the skipped bisect): render-time `on_focus`/`on_blur` registration
WORKS — the deferred activation rides the normal effect cycle; the
earlier claim that it never activates was a misdiagnosis (the failing test
turned on harness composition: an off-screen input never enters the
painted frame's focus path, and test windows start inactive, their focus
event paths empty until activate_window). The landed design registers
idempotently from the workspace render — keyboard focus arrivals covered;
the harness facts live in the focus test's comments. The false mechanism
claim also rode into the pushed a23af25a8d message; 883971c07d and this
note are the erratum.

**Design:** client-side lexical search over pane blocks (the open transcript is
materialized in the pane — no server dispatch); 150 ms debounce via
`cx.background_executor().timer` (NEVER `smol::Timer` — `.rules`); focus in the
search box suspends follow (manual-scroll precedent), blur re-enables.

**Acceptance:**
1. Typing "dunning" into the Dunning transcript finds all matches; Enter cycles;
   current match visibly marked (interaction test).
2. Zero-match shows an honest "no matches" state, not silence.
3. Match styling composes with highlight and strike overlays (field-wise unit test).

**Replaces/deletes:** nothing.

**Risks:** 14,270-word transcripts — debounce + incremental scan window (visible
blocks first), full scan budgeted at ≤ a few ms per keystroke burst.

### S4 — Playback speed (P1)

**Objective:** speed control cycling 0.5×/1×/1.25×/1.5×/2× on the shared transport;
works on audio and video; position and marks labels stay correct.

**Anchor:** study §6 P1-4; Reduct 1.25×, Descript — table stakes in every reference
product.

**Touchpoints:** transport render (`media_widget.rs` ~:894-1100 region), audio
(`audio_player.rs` — verify rodio `Sink` speed support at slice start),
video pacing (`start_playback_loop` per reference.md → `media_widget.rs:575`;
`DECODE_POLL_INTERVAL`/`DISPLAY_POLL_INTERVAL`, `video_decoder.rs:51,54`),
position accessor (`transport_position_secs:1100`).

**Design:** speed state on the widget; audio via rodio sink rate; video via
deadline scaling (frame due-time divided by rate) in the pacing loop; the pacing
math extracted as a pure function for unit pinning.

**Acceptance:**
1. Pure-function unit test: frame deadlines scale with rate (1.5× → 2/3 interval).
2. Audio: position advances ~2× wall-clock at 2× within tolerance (timer-driven
   test, `background_executor().timer`).
3. Benchmark regression: `MIN_PRESENTED_FRAMES_AT_30_FPS` unaffected at 1×.

**Replaces/deletes:** nothing.

**Risks:** rodio rate API availability — verify FIRST at slice start; if absent,
decode-side resampling is larger and the slice re-scopes with a written note (the
honest-degradation rule: never ship video-only speed labeled as speed).

### S5 — In-panel transcription correction (P1)

**Objective:** select misheard words → "Suggest correction" in the popover → type
the replacement → writes a targeted **correction layer** through
`educt_store_layer` (kind=correction), then `educt_realign_transcript` for
cardinality changes; the working view refreshes; source words are never mutated
(the discipline).

**Anchor:** study §6 P1-5; Descript write-mode / Reduct inline fix, adapted to the
layer discipline; the study's §4 Web Annotation brittleness note is exactly why
corrections stay layers over immutable source words.

**Touchpoints:** popover (S2 dependency); server tools unchanged
(`educt_store_layer`, `educt_realign_transcript`, `educt_apply_corrections`);
single edit admission (`begin_edit` rejects overlap, `media_viewer.rs:1250`);
alignment note machinery (five-state gate, `transcript_alignment_note:8425`).

**Design:** the correction editor is the label-editor pattern with a replacement
field; dispatch chain under the existing edit admission; success reloads the
transcript request and the alignment note updates; overlapping prior corrections
supersede on the range (mirror `educt_retranscribe_section`'s listed-supersession
semantics — read the server's correction-layer contract at slice start and pin the
payload schema with a fixture).

**Acceptance:**
1. Scripted-invoker test: correction over words [a..b] appears in the working view;
   source bundle unchanged.
2. Cardinality-changing correction chains `educt_realign_transcript` and the
   alignment note reads `re_anchored` (the Dunning recording is already
   `re_anchored` — a live second correction exercises the path).
3. Edit-in-flight refusal extends the existing admission test.

**Replaces/deletes:** nothing (adds the affordance the agent-mediated
`educt_correction_pass` already provides as data paths).

**Risks:** correction-layer payload schema fidelity (server contract first);
selection-to-source-index translation through the reanchored index map (the
`.rules` retranscribe trap — working-view selections MUST be translated).

### S6 — Reel composer (P2)

**Objective:** the reel becomes an editable object — an ordered clip list built
from included highlights (+ whole-recording mode), drag reorder, per-clip
include/exclude, in-reel strike (sub-range cut within a clip), duration readout,
and an auto-seek preview that plays the composed order without rendering; compose
and render then flow through the existing EDL path.

**Anchor:** study §6 P2-6; Reduct's verified reel pattern (drag in, reorder,
strike-through-in-reel edits the video).

**Touchpoints:** `compose_reel_params:9351`, `union_word_ranges:8898` (gains an
explicit order parameter), `compose_workspace_reel:3703` (store_layer → render_edl
chain), reel section (`render_workspace_reel_section:7443`), highlights/cuts
sections (`:7210`, `:7360`), `pane_reel_compose` provenance identity.

**Design:** composer state = ordered `Vec<ClipSpec>` (highlight identity + word
range + include flag + in-reel strike sub-ranges); op builder = ordered, per-clip
difference math for strikes; preview = auto-seek chain on the shared player
(span end → seek next span start — the follow-loop's task pattern); render
unchanged (lossless, ordered ops). Title blocks are a declared stretch item
(render-time `video_add_caption` overlay) — not in S6's acceptance.

**Acceptance:**
1. Unit test: reorder changes composed op order; strike difference math covers
   contained/overlapping/boundary cases.
2. Preview plays spans in composed order (scripted-widget interaction test).
3. Compose→render still lands a playable asset via display-hint ingestion; the
   composer EDL never parses as a researcher strike (identity pin extended to the
   ordered form).
4. End-to-end on the Dunning recording: highlights → order → in-reel strike →
   render → playable reel in the Library (operator-verifiable deliverable).

**Replaces/deletes:** the unordered `union_word_ranges` composition is replaced by
the ordered builder (delete the unordered call sites in the same change — P5.5).

**Risks:** size L — the plan's only L; seek jumps across an 82-minute recording in
preview (decoder seek performance; measure before UX polish); op-order must not
leak into strike identity.

**Addendum (2026-11-06 — the composition review's S6 design pre-review,
`kask/docs/research/media-panel-composition-interaction-review-2026-11-06.md`
§5, commit `7a1306ebb5`; operator-approved this date). The review's
findings-on-paper fold into this slice's spec:**

- **Membership is replaced, not paired (F1):** `reel_selection` and its
  three write paths (`popover_include_in_reel`, the highlight-row toggle,
  and the reload rebuild in `apply_transcript_result`) are deleted — the
  ordered `ClipSpec` list's include flag is the one source of truth for
  membership and order (no set-and-list pair).
- **Composer state survives reloads (F2):** every annotation CRUD reloads
  the workspace, and the landed reload rebuilds membership from all
  researcher highlights — S6 keys composer state (order, inclusion,
  strikes) to highlight identity and re-matches across reloads. New
  acceptance test: order/exclude/strike, perform an unrelated annotation
  edit, assert composer state survives the reload.
- **In-reel strikes bake into Keep ops (F3):** the server unions ALL Cut
  ops globally (`transcript_select.rs` `edl_to_keep_ranges`), so a
  per-clip strike is never a Cut in the composer EDL — the op builder
  splits the one clip's Keep op via per-clip difference math that REUSES
  the server's algebra (publish `subtract_cuts_from_range` or add a pub
  per-clip helper beside it; the S5 `merge_over_prior` reuse is the
  standard — no fork). Acceptance-3 additionally pins that a composer
  EDL carries no Cut ops.
- **Preview timing reads the render path's source (F10):** the preview's
  word→time mapping uses the stored source bundle words
  (`edl_to_clip_plan`'s input), never the working view's timings —
  preview and rendered reel must agree on `re_anchored` views. New
  acceptance test: preview over a `re_anchored` fixture asserts each seek
  equals `edl_to_clip_plan`'s output for the same clip.
- **Union mirror deleted (F4):** `union_word_ranges` (one call site,
  inside `compose_reel_params`) is deleted; union, where still needed,
  calls the server's pub `union_ranges`.
- **Preview substrate (F9):** the preview reuses the follow-loop task
  pattern (single-instance guard, GPUI-native timer, exit on surface
  change, poll only while playing, position through
  `transport_position_secs` — the S4 rate contract), joins the teardown
  sites (surface-switch, open, close, reload) and exits on widget-None
  (hidden media suspends); the 82.7-minute seek jump is measured before
  any UX polish.

**Landed (2026-11-06, commit `01374f55ea`):** the ordered `ReelClip` composer
replaces `reel_selection` and its three write paths and the
`union_word_ranges` mirror (F1/F4); composer state re-matches to highlight
identity on every reload (F2, pinned red-first by
`reel_composer_rebuilds_reorders_and_survives_reload`); in-reel strikes bake
into Keep ops through the server's published `subtract_cuts_from_range`
(F3, pinned by `in_reel_strikes_bake_into_keeps_not_cuts` — a composer EDL
carries no Cut ops — plus the server-side boundary pins); the preview reads
the SOURCE bundle words (F10, pinned by
`reel_preview_plan_reads_source_bundle_words` over a re_anchored fixture)
and plays the composed order (`reel_preview_plays_spans_in_composed_order`,
scripted-widget over the committed `reel-preview.wav` fixture; teardown
pinned by `reel_preview_teardown_joins_the_surface_lifecycle`). Overlapping
clips keep only the later clip's unplayed remainder (the algebra admits
each source span once). Gates at landing: media_panel 132 passed
(126 + 6 new pins), hkask-mcp-media lib 493 (491 + 2 boundary pins) /
integration 162 / doctest 1, both clippy gates exit 0; the citation ladder
clean (the 8 adjudicated pairing artifacts at shifted values); the
duplication baseline bumped in the earning commit (5635: the inherited
5608 red plus S6's share). End-to-end server half driven live on the Dunning
recording (transcript `5509bb94`): two researcher highlights → ordered
compose [B(2330-2350) with strike 2340-2342 baked, A(150-170)] → store
(layer `b3571906`, three Keep ops, no Cuts) → render (asset `04c271b1`, 3
clips in composed order, the strike gap visible, playable 14.65s WAV);
ffmpeg seek probes on the 82.7-min source measure 0.03-0.06s at offsets up
to 40:00 (decode-layer seek cost is flat — no UX risk; the live-UI seek
latency stays the operator's measurement). reference.md v3.5.0 carries the
landed contracts.

### S7 — Standards exports (P2)

**Objective:** highlights export as W3C Web Annotation JSON (TextPositionSelector +
TextQuoteSelector dual-anchored, motivation `highlighting`/`tagging`, creator
`researcher_supplied`); WebVTT export with word-level karaoke cues beside SRT;
composed reels serialize to OpenTimelineIO (stack of clips, source ranges, media
references).

**Anchor:** study §4 (the standards mapping); W3C REC 2017-02-23; MDN WebVTT
karaoke timestamp tags; ASWF OpenTimelineIO.

**Touchpoints:** server — `kask/mcp-servers/hkask-mcp-media` (extend `educt_export`
formats: `webvtt`, `web_annotation`; OTIO serialization as a new export family over
EDL layers); panel — export affordances in the highlights/reel sections and the
inspector's Reel tab (`InspectorTab::ALL`, `media_viewer.rs:603`).

**Design:** serializers are pure functions over stored layers/transcripts —
deterministic, fixture-pinned byte-for-byte (no Python anywhere — `.rules`); OTIO
validation is structural against the documented schema keys in Rust, with the full
round-trip honestly deferred to operator-side tooling. Exports land outside the
playable gallery (the durable-export pattern SRT already uses).

**Acceptance:**
1. Fixture tests per serializer: Web Annotation JSON carries both selectors and
   motivations; WebVTT cue parses with karaoke tags; OTIO clip keys match schema.
2. Panel export dispatch produces the artifact (scripted-invoker test) and
   surfaces the path.

**Replaces/deletes:** nothing — SRT export remains; WebVTT is additive.

**Risks:** `educt_export` signature change is a server API change — the panel's
export request lifecycle consumes the new format enum at the same seam; keep the
export surfaces away from any QA/training path (they are operator recordings, but
the export code must not enter the Brave-origin exclusion discipline's blind spot).

## 5. Verification discipline (all slices)

- Pin test first (red) → fix → green; the oracle that adjudicated a claim re-runs
  against the change claiming it — no inherited green (`.rules`).
- `./script/clippy` over every touched crate per slice; plain `cargo check` is not
  a clean claim; `--test-threads=1` for any live-mutation probes.
- GPUI traps: no `block_on` foreground; timers via `background_executor().timer`;
  `AsyncApp` not `Send`; one state-mutating kanban call per task per batch.
- Net lines + replaces/deletes reported per slice; `.rules` additions proposed in
  the change report (never edited inline).

## 6. Governance

On operator confirmation:
1. Register the goal via `kanban_goal_create` — criteria C1–C7 frozen from this
   plan §2; source locator requires the session/interaction identity (never
   invented) — recorded as the open intake gate.
2. Create a board "Media panel transcript bundle" with slices S1–S7 as tasks
   (criteria copied per task); kata state on the active task; phase gates via
   `kanban_goal_judge` with per-criterion evidence.
3. Slices are delegation-shaped (self-contained, acceptance-named) — spawnable
   agents with disjoint write scopes per slice; S1 stays with the operator's own
   stream given the live repro.

## 7. Evidence appendix

- Study (mechanism, patterns, standards, priorities):
  `kask/docs/research/media-transcript-interaction-study-2026-11-04.md` §1–§8.
- Landed-surface contract: `kask/docs/diataxis/media_panel/reference.md` (verified
  2026-11-02; sections cited per claim above).
- Predecessor plan: `kask/docs/plans/media-panel-redesign-plan-2026-11-02.md`.
- Research ledger: run `ce75b8fc6a8392f5` (14 artifacts, 13 domains, mean
  confidence 0.84; sources listed in the study §9).
- File:line inventory: as cited per slice; all panel line references are from the
  reference model's verified alignment (2026-11-02) and this run's direct reads —
  re-verify lines at slice start (line-shifting edits invalidate stale cites).