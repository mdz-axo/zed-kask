---
title: "media panel — Study: synced-transcript interaction patterns, emerging standards, and the educt bundle build plan"
audience: [developers, architects]
last_updated: 2026-11-05
version: "1.0.0"
status: "Active"
domain: "Media panel / UI"
mds_categories: [domain, composition]
---

# media panel — Study: synced-transcript interaction patterns, emerging standards, and the educt bundle build plan

title: Media transcript-bundle interaction study
audience: operator (PI) + build agents
last_updated: 2026-11-04
status: proposed — research complete, design refinements pending operator confirmation
research_run: ce75b8fc6a8392f5 (completed; 14 artifacts over 13 distinct domains, mean evidence confidence 0.84, stable under weight-profile substitution)
scope: `crates/media_panel` transcript sub-surface, `hkask-media-widget`, the educt transcript tools, and the Reduct reference model (`zk-ref-open` reduct-transcript-model)

## 1. Triggering defect — the close-transcript overlap

**Report (operator, 2026-11-04):** after hitting "← Close transcript", the player
buttons are superimposed over another menu — overlapping artifacts in the viewer.

**Diagnosis (code-grounded; pinned by a proposed layout test, not yet machine-reproduced).**
The two surfaces render the shared player widget under different clipping contracts:

- The transcript sub-surface renders the player inside a bounded, clipped row:
  `div().h(px(140.)).flex().min_w_0().overflow_hidden().child(widget)`
  (`render_workspace_header_item`, `crates/media_panel/src/media_viewer.rs:6022`).
- The plain asset workspace renders it inside
  `div().id("media-viewer-content").flex_1().min_h_0().p_3().child(widget)`
  (`render_workspace`, `crates/media_panel/src/media_viewer.rs:4575-4580`) — **no clip**,
  and no clip again at the workspace root (`:4589`) or the viewer pane
  (`crates/media_panel/src/media_panel.rs:363-370`).

The widget root carries a `min_h(px(240.0))` floor when no error is shown
(`crates/hkask-media-widget/src/media_widget.rs:1040`), justified for indefinite-height
content-flow containers. In a short pane — and the edit toolbar alone is ~800px of
buttons that wraps to 2–3 rows at narrow docks (`render_edit_toolbar`,
`crates/media_panel/src/media_viewer.rs:4615-4620`) — the content area can fall below
240px. The widget root then lays out 240px tall in a smaller container: the transport
(play/seek/position, at the layout's bottom) paints past the content bottom, past the
unclipped workspace root, past the unclipped viewer pane, and over the panel split into
the Steer director below. The transcript's 140px row masked the same defect because
`overflow_hidden` clipped the escape; closing the transcript was the first render of
the unclipped path (the operator reached the workspace through
open-transcript-from-Library, so the plain workspace with the loaded video had not
rendered before).

**Falsifier (the pin test, P0-1):** a GPUI layout test in the `viewer_layout_tests`
style — render the workspace in a short pane, close the transcript, assert via
`debug_bounds` that the `media-widget` bounds stay inside the viewer pane. Green there
while the operator still sees overlap would falsify this mechanism and redirect to
GPU surface caching after the rebuild.

**Proposed fix, two tiers:**
- P0 (surgical): `overflow_hidden()` on the workspace content container (or the
  workspace root). Degradation becomes visible clipping, never corruption of a
  neighboring surface.
- Follow-up (root): make the 240px floor conditional on indefinite-height containers
  (its documented purpose) so a definite-height pane can shrink the widget; re-run the
  widget layout tests and the playback benchmark (decode path untouched).

**Adjacent hygiene (same build item):** `close_transcript_workspace`
(`crates/media_panel/src/media_viewer.rs:2454-2470`) clears nearly all sub-surface
state but, unlike `open_transcript` (`:2384-2443`), does not reset `selection_mode`
and the list state — a latent asymmetry the reel-composer work must not inherit.

**Resolution (2026-11-04, S1, commit `828b9c09f7`):** both proposed tiers landed the
same day. The workspace content container clips (`overflow_hidden()`,
`crates/media_panel/src/media_viewer.rs:5121`) and gained the `media-viewer-content`
layout-test hook; the 240px usable-height floor moved to the content-flow embedder
(`crates/hkask-viz-core/src/hkask_viz_core.rs:383`), and the widget site now documents
the move (`crates/hkask-media-widget/src/media_widget.rs:1049-1063`);
`close_transcript_workspace` now resets `selection_mode`
(`crates/media_panel/src/media_viewer.rs:2519`), pinned by
`closing_transcript_leaves_widget_inside_pane`. The pre-fix observations above —
the widget-root floor (`media_widget.rs:1040`), the unclipped content container
(`media_viewer.rs:4575-4580`), the workspace-root view (`:4589`), and the
`selection_mode` asymmetry — describe the state the fix landed against.

**Follow-up (2026-11-04, same defect family):** the workspace player still lost its
transport — play/stop buttons and the seek slider — at moderate pane heights
(operator report: the slider is absent in the workspace player while the same
chat-thread player keeps it). The §7.2 discipline was violated INSIDE the widget:
the video area carried `min_h(px(120.))` (and the image area `min_h(px(100.))`), so
once the content area fell below the column's min-content (~154px: floored video +
transport + gap + border), the transport painted past the clipped bottom — measured
at a 56px host: transport laid out at y=125..159, entirely outside the box, so the
slider read as missing rather than cut. Both floors are removed (`min_h_0`, the
video and image arms of `crates/hkask-media-widget/src/media_widget.rs`):
definite-height panes shrink the media area to the space left after the transport
(the transport is pinned inside the box at every host height), and the content-flow
embedding needs no floor — the decoded frame's aspect derivation against the
definite flow width holds the video open (measured 798x449 for a 320x180 frame at
an 800px width; only the transient pre-decode placeholder shrinks to text height).
Pinned by `transport_stays_visible_at_every_host_height`,
`flow_embedding_keeps_decoded_video_open`, and
`flow_placeholder_does_not_collapse_the_player` (widget `layout_tests`); the §7.2
rule is now held by the widget itself.

## 2. The landed surface (verified against the tree)

The 2026-11-02 redesign (phases A–D) holds the reference model's synced recording:
import → organize → transcribe → read along → highlight → cut → compose and render a
reel → play the reel, entirely in-panel
(`kask/docs/diataxis/media_panel/reference.md` L465-517). Word-click seek, 4 Hz
follow-along with manual-scroll suspend and Follow re-enable, two-click range
selection → label editor → highlight CRUD, strikes with line-through and Restore,
three compose modes (highlights / whole-with-cuts / highlights-with-cuts) chaining
`educt_store_layer` → `educt_render_edl`, per-recording transcription status, the
events surface, inspector annotations CRUD. The earlier interface-gap analysis
(2026-10-25, memory) predating this surface is superseded.

## 3. What the reference products actually do (evidence base)

- **Reduct** (primary help docs + independent review):
  - Word-timestamped transcript; click any word → jump (we match).
  - Select text → action popover (highlight, tag, clip, comment, download, SEE MORE)
    with keyboard shortcuts — selection is drag-select "like a word processor".
  - Project funnel: Recordings → Highlights → Tags → Reels; tags are #hashtags on
    highlights, sortable/groupable; "Convert to Reel" everywhere; reels accept
    highlights, labels/label groups, entire recordings, other reels; drag-and-drop
    reorder; **striking through text inside a reel edits the video**; title blocks;
    inline playback of the reel before export; share links; MP4/WAV export; FCP XML
    and Premiere-panel conform.
  - Player: timeline+timestamp scrub, CC, playback speed (1.25x), quality, fullscreen,
    pop-out, corner-drag resize.
- **Descript**: the transcript IS the timeline — deleting words cuts media; undo
  restores; a separate write mode corrects transcription; one-click filler removal;
  "your text is another way to visualize your audio and video".
- **Usage evidence (MeetScript, arXiv:2309.12115):** in live transcript annotation,
  low-effort interactions (highlight, like) dominate; edits are rare under time
  pressure. Design consequence: the high-frequency path (select → highlight) must be
  one gesture; everything heavier can be one click deeper.
- **Academic lineage:** text-based editing predates the products — a 2012 paper
  describes deleting transcript text to edit interview footage (per the Loopdesk
  secondary source; the title resolution for "Knotty" in this run returned an
  unrelated work, so no DOI is cited — treat the 2012 attribution as
  corroborated-secondary). Direct ancestors verified this run: Non-Linear Editing of
  Text-Based Screencasts (UIST 2018) and Computational Video Editing for
  Dialogue-Driven Scenes (SIGGRAPH 2017).

## 4. Emerging standards and how they map onto educt

- **W3C Web Annotation Data Model** (REC 2017-02-23): an Annotation is body+target
  with motivations `highlighting`, `tagging`, `bookmarking`. Our researcher highlights
  ARE the model's pattern: a word-range highlight is a
  `TextPositionSelector` (start/end), and the excerpt we render in the highlights list
  is the `TextQuoteSelector` (exact/prefix/suffix). The model's brittleness note —
  position selectors break when the resource changes, so anchor to a state — is
  exactly our immutable-source-words + correction-layer + re-anchored-index-map
  discipline. Application: export highlights as Web Annotation JSON with BOTH
  selectors (position + quote), motivations `highlighting`/`tagging`, creator
  `researcher_supplied`.
- **WebVTT karaoke timing**: intra-cue timestamp tags are the standard form of
  word-level timed text and paint-on highlighting — our follow-along word mark is the
  same pattern in GPUI. Application: a `webvtt` export format (word cues via karaoke
  tags) beside SRT; future captions-in-player can reuse it.
- **OpenTimelineIO / EDL interchange**: OTIO (ASWF, Pixar-origin) is the open
  interchange for editorial cut information — order and length of cuts, tracks,
  markers, external media references; adapters to FCP7 XML, AAF, CMX 3600. Reduct's
  FCP-XML/Premiere conform is this idea in product form. Application: serialize a
  composed reel EDL to OTIO (a Stack of Clips with source ranges referencing the
  recording) — our `Keep` ops are already a minimal stack.

## 5. Interaction-principles base

- Direct manipulation (Shneiderman 1983/1997): continuous representation, rapid
  reversible incremental actions, immediate feedback — comprehensible, predictable,
  controllable. Verdict: our click-to-seek and follow-along comply; our two-click
  range + row actions are close, but drag-select and keyboard shortcuts are the
  missing gesture layer, and reel composition is not yet direct (no reorder).
- Progressive disclosure (Reduct's popover + SEE MORE): heavy actions sit one step
  behind the selection.
- Ambient process state (badges/drawer, never a destination) — already the panel's
  documented model, matching FCP background tasks.

## 6. Gap analysis → prioritized refinements

P0 — correctness
1. Close-transcript overlap: pin test + `overflow_hidden` (§1); conditional 240px
   floor follow-up; `close_transcript_workspace` teardown symmetry.

P1 — interaction parity with the reference model
2. Drag-select words (continuous gesture) + selection popover (Highlight / Cut /
   Copy time / Include in reel) with keyboard shortcuts; the two-click range stays as
   the fallback. Grounding: Reduct's verified selection pattern; Shneiderman; the
   MeetScript low-effort-dominance finding.
3. In-transcript search box (lexical over the working view, jump-to-match,
   match highlighting, bounded by the existing `educt_search_transcripts` semantics).
4. Playback speed control (0.5×–2×) on both media kinds. Table-stakes in every
   reference product; currently documented absent.
5. In-panel transcription correction: select misheard words → "Suggest correction" →
   the existing `educt_correction_pass` (gated, layered, never mutating source words),
   with `educt_realign_transcript` follow-through surfaced. Grounding: Descript write
   mode / Reduct inline fix, adapted to our correction-layer discipline.

P2 — the reel becomes an editable object
6. Reel composer surface: ordered clip list (drag reorder — `union_word_ranges` gains
   an explicit order), per-clip include/exclude, in-reel strike-through (Reduct's
   verified reel pattern), title blocks as label-only clips, duration readout, inline
   play of the composed order before render. Compose provenance stays
   `pane_reel_compose` so composer EDLs never parse as researcher strikes.
7. Label/tag index across recordings (the Tags funnel): inspector-level view grouping
   highlights by label across the gallery; "compose reel from label".
8. Standards exports: Web Annotation JSON for highlights (§4), WebVTT word-cue export
   beside SRT, OTIO serialization of composed reels (interchange into NLEs).

P3 — player affordances: fullscreen, pop-out/float, corner-resize (all documented
absent; lowest research priority — reference products have them, our operator tasks
have not demanded them).

## 7. GPUI build discipline (transferable to the panel skill)

1. Clip at the pane boundary that hosts intrinsically-sized content: every shrinking
   container (`min_h_0`) hosting min-content children needs `overflow_hidden`, or the
   children's floors paint into neighbors. The 140px transcript row had the clip; the
   workspace content did not — the defect in §1 is this rule's violation.
2. min-height floors belong to indefinite-height containers only; in definite-height
   shrinking panes they fight the `min_h_0` discipline and must either yield or be
   clipped.
3. Ship a `debug_selector` on every interactive region and a `debug_bounds` layout
   test for every transition between surfaces (the pattern now covers `media-widget`,
   `media-video-area`, `media-transport`, `media-viewer-content` and
   `simple-slider-track`; the close-transcript transition's missing selector was
   the gap that let §1 ship).
4. Absolute children only under an explicit `relative()` anchor
   (`simple_slider.rs:107` is the positive example).
5. State teardown symmetry: every state an open path resets, the matching close path
   resets too — asymmetries are latent traps for later surfaces (§1 hygiene).

## 8. Proposed goal — completing the synced-recording build

Goal text (operator's words, to be confirmed): "The media panel's synced recording —
transcript bundle through reel — is complete and defect-free against the Reduct
reference model."

Observable criteria (draft for confirmation):
1. Closing the transcript sub-surface renders zero overlap in any pane height, pinned
   by a layout test over the close transition.
2. Word selection supports drag-select plus a selection popover with keyboard
   shortcuts, beside the existing two-click range.
3. The transcript sub-surface offers in-transcript lexical search with jump-to-match.
4. Playback speed control works on audio and video, position/marks labels coherent.
5. The reel composer edits order, inclusion, in-reel strikes, and titles, then composes
   and renders through the existing EDL path.
6. Highlights export as W3C Web Annotation JSON (position+quote selectors) and composed
   reels serialize to OpenTimelineIO.

Registration is blocked on the session/interaction identity (per goal-intake
discipline, the source locator is never invented) — confirm the target and provide or
approve the session identity, and the goal plus a P0 build task are created.

## 9. References (ledger-recorded, run ce75b8fc6a8392f5)

- Reduct Help Center — Video Text Interface: https://help.reduct.video/en/articles/video-text-interface
- Reduct Help Center — Quick Start Guide: https://help.reduct.video/en/articles/quick-start-guide
- Reduct — Online Video Editor (blog): https://reduct.video/blog/online-video-editor/
- Scott Simmons, ProVideo Coalition — Review: Reduct.video (2022-10-31):
  https://www.provideocoalition.com/review-reduct-video-a-cloud-and-ai-based-system-to-transcribe-organize-correct-and-edit-video-from-transcripts/
- Descript — AI Video & Audio Text-based Editor: https://www.descript.com/
- Loopdesk — Text-Based Video Editing Explained: https://loopdesk.ai/blog/text-based-video-editing-explained
- W3C — Web Annotation Data Model (REC 2017-02-23): https://www.w3.org/TR/annotation-model/
- MDN — Web Video Text Tracks Format (WebVTT): https://developer.mozilla.org/en-US/docs/Web/API/WebVTT_API/Web_Video_Text_Tracks_Format
- Academy Software Foundation — OpenTimelineIO: https://github.com/AcademySoftwareFoundation/OpenTimelineIO
- Park, Park, Oh — Non-Linear Editing of Text-Based Screencasts (UIST 2018), slides:
  https://uilab.kr/assets/research/UIST2018/uist2018_park_slides.pdf
- Leake, Davis, Truong, Agrawala — Computational video editing for dialogue-driven scenes (SIGGRAPH 2017):
  https://www.lanfanshu.com/paper/61e5097a31d3b89581f623f4
- MeetScript (arXiv:2309.12115): https://arxiv.org/html/2309.12115v1
- Shneiderman — Direct Manipulation for Comprehensible, Predictable and Controllable
  User Interfaces: https://www.cs.umd.edu/~ben/papers/Shneiderman1997Direct2.pdf
- Direct manipulation interface (overview): https://en.wikipedia.org/wiki/Direct_manipulation_interface

Integrity note: the attempted `resolve_paper` for "Knotty: A Tool for Editing
Structural Video" returned an unrelated typography paper (doi:10.21275/sr25726194656);
that DOI is rejected and not cited. The 2012-origin claim rests on the Loopdesk
secondary source only.