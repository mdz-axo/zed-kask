# Reference Model: Video Widget with Editing and Concatenation

Status: research specification — anchor, benchmark, and improvement target for
the media viewer's interactive editing features (todo V1/V2). Researched
2026-08-29 against the canonical open-source implementations.

## Purpose

Every design decision in our trim/concat UI should be traceable to one of:
(a) a pattern the reference implementations converge on, (b) a documented
tradeoff they resolved, or (c) a deliberate improvement we can defend. This
document is the anchor that makes "is our design good?" answerable — without
it, "works" is the only quality bar.

## The reference implementations

### 1. LosslessCut — primary UX benchmark
https://github.com/mifi/lossless-cut (GPL-2.0, 43k stars)

The canonical minimal lossless trim/merge tool, built on FFmpeg + a browser
video player. Its design decisions, from the README and docs:

- **Segments, not in/out pairs.** A file has a *list* of segments
  (start/end), each labeled and taggable. One file → many cuts. This is the
  single biggest gap in our current design (we hold one in/out pair per
  asset).
- **Lossless stream-copy cutting** (`-c copy`): near-instant, no quality
  loss, but cuts are **keyframe-aligned** — the cut point snaps to the
  nearest preceding keyframe. "Smart cut" (experimental) re-encodes only the
  partial-GOP edges for frame-precise lossless-ish cuts.
- **Concatenation precondition**: lossless merge works on "arbitrary files
  with identical codecs parameters, e.g. from the same camera." Mixed codecs
  require re-encode. The tool surfaces this rather than silently producing
  broken output.
- **Rearrangeable segment order** — the segment list is the edit decision
  list (EDL), reorderable before export.
- **Preview/render separation**: marking segments is instant metadata; the
  FFmpeg render happens once, on demand, and a **command log** shows the
  exact FFmpeg invocation (inspectable, re-runnable).
- **Undo/redo**, per-project segment persistence, EDL import/export (CSV,
  XML for DaVinci/FCP, YouTube chapters, CUE).
- **Keyboard-first workflow** (I/O-style marking, frame/keyframe jumping,
  timeline zoom).
- **Timeline aids**: video thumbnails, audio waveform, scene/silence
  detection.

### 2. MLT Framework — API/architecture model
https://www.mltframework.org/docs/framework/ (behind Shotcut and Kdenlive)

MLT formalizes the editing model our UI sits on top of:

- **Producer with `in`/`out`/`length` properties** — every source carries
  its own in/out points as first-class state.
- **Playlist = sequential concatenation of in/out cuts**:
  `mlt_playlist_append_io(producer, 0, 99)` appends a *range* of a producer.
  Concatenation is not "join files" — it is "append (producer, in, out)
  triples." Our concat queue stores bare srcs; the reference model says it
  should store (src, in, out) triples so a queued item can itself be a trim.
- **Tractor/multitrack** for parallel tracks and transitions — out of scope
  for us, but the boundary is informative: single-track playlist editing
  (MLT's own docs call the multitrack NLE experience "bombastic, confusing
  and ultimately frustrating" for simple jobs) is the right scope for a
  viewer-integrated editor.
- **Attached filters** stay with the cut through reordering — the reason our
  trim results should carry provenance (they do, via display_hint).

### 3. GStreamer Editing Services (GES) — timeline object model
https://gstreamer.freedesktop.org/documentation/ges/

The other canonical editing library. Its object model: `GESTimeline`
contains `GESLayer`s containing `GESClip`s; each clip has `start`,
`in-point`, and `duration`. **Trimming is just mutating those numbers** —
the render is a projection of the timeline, never a destructive operation
on sources. The lesson for us: marks and the concat queue are a *timeline
document*; `video_clip`/`video_concat` are *renders* of it. The document
should be inspectable and editable before any render is dispatched.

### 4. Avidemux — the minimal interaction
http://avidemux.sourceforge.net/

The classic two-marker cut: mark A, mark B, delete/save selection. Proof
that a single in/out pair is a legitimate v1 interaction — but LosslessCut's
segment list is where every serious tool converges.

### 5. Reduct.video — the transcript-as-EDL paradigm
https://reduct.video (commercial; researched as interaction model, not code)

Reduct inverts the timeline: **the transcript is the timeline** ("Ctrl+F for
video"). Its design decisions, from the product site:

- **Interactive transcript**: click a word → playback jumps to that moment.
  **Selecting text selects video** — a text range IS a media range. The
  transcript is the primary navigation and selection surface, not a
  scrubber.
- **Strikethrough editing**: in a composition (their "Reel"), select text →
  "cut" → that passage is skipped in playback. Editing = deleting words.
  Filler and digression removal becomes a text operation.
- **Highlights → Reels**: highlight transcript passages, label/tag them by
  theme, assemble highlights into a Reel. The EDL is a sequence of
  transcript selections, each with word-level timestamps.
- **Repository-wide semantic search**: fuzzy/NLP search across every
  transcript — find moments by meaning, not scrubbing. "Find where he
  talks about Cinderella" is a first-class operation.
- **Labels/tags** on highlights for thematic organization; **Videoboard**
  (2D canvas) for arranging highlights into affinity maps/storyboards.
- **Link to selection**: a shareable URL to the exact transcript moment.
- **Transcript correction** (select error → fix / replace-all) — the
  transcript is editable metadata, not immutable output.
- **Redaction** of PII (names, faces, screen content) — enterprise feature.
- **Premiere Pro round-trip**: rough-cut in Reduct → Premiere sequence using
  original source files. Rough cut here, polish there.

**Why this matters for us specifically:** our media server already ships
`transcribe` / `transcribe_bundle`, and our editing surface is a
conversation. Reduct's paradigm maps onto our stack almost directly:

```
transcribe (word timestamps) → search/select text → (start, end) →
video_clip → concat queue = the Reel
```

And it inverts our improvement target: where LosslessCut's UI is the only
entry point, Reduct still requires the human to read and select. **An agent
that takes "find where he explains the Cinderella curve and clip it to a
30-second reel with the Hamlet part" and produces the selections + renders
is Reduct's paradigm with the selection engine replaced by the conversation**
— transcript search and selection become natural-language operations over
the same governed tools.

The strikethrough model also names an operation our timeline tools don't:
**subtractive editing** ("cut all the filler") = transcribe → identify
unwanted ranges → render the complement → concat. The agent is unusually
good at exactly this classification step.

## Extracted interaction patterns (the design vocabulary)

| Pattern | Source | Our status |
|---|---|---|
| Mark in/out at playhead | Avidemux, all NLEs (I/O keys) | ✅ have (Mark In/Out buttons) |
| Multiple segments per file | LosslessCut | ❌ **gap — single pair only** |
| Segment list as reorderable EDL | LosslessCut, GES | ❌ gap (queue is append-only) |
| Queue entries are (src, in, out) triples | MLT playlist | ❌ gap (queue stores bare srcs) |
| Stream-copy trim, keyframe-aligned | LosslessCut | ✅ have (`-c copy` in `video_clip`) |
| Frame-precise "smart cut" option | LosslessCut (experimental) | ❌ future |
| Concat requires matching codec params | LosslessCut | ⚠️ unverified — server should check and surface |
| Preview instant / render async + status | all | ✅ have (dispatch + status line) |
| FFmpeg command log | LosslessCut | ❌ gap — easy, high trust value |
| Undo/redo | LosslessCut, NLEs | ❌ future |
| Keyboard shortcuts for marking | all | ❌ gap (buttons only) |
| Timeline thumbnails/waveform | LosslessCut, NLEs | ❌ future |
| Project/EDL persistence | LosslessCut, MLT XML | ❌ future (workflow_save exists server-side) |
| Non-destructive: sources never modified | GES, MLT | ✅ by construction (renders write new files) |
| Transcript as timeline: text selection = media range | Reduct | ❌ gap — `transcribe` exists server-side, unconnected to editing |
| Subtractive editing (strikethrough: cut the unwanted) | Reduct | ❌ gap — agent-classification + complement render |
| Semantic search over transcript repository | Reduct | ❌ gap — `transcribe` + search would compose |
| Word-click → seek | Reduct | ❌ gap — transcript UI absent |
| Labels/tags on selections | Reduct, LosslessCut | ❌ gap |
| Shareable link to a moment | Reduct | ❌ out of scope (no sharing surface) |

## Benchmark checklist (assessable criteria for V1)

1. Trim a 7:49 video to a 30s range: **< 2 seconds wall time** (stream copy
   makes this achievable; a re-encode would take minutes — if our trim is
   slow, we regressed from the reference mechanism).
2. Trimmed clip **plays immediately** in the viewer after the dispatch
   returns (auto-surfaced via display_hint).
3. Cut lands **within one keyframe interval** (~2-10s for our sources) of
   the requested marks — keyframe alignment is the documented tradeoff, not
   a bug; the UI should say so at the mark ("cuts snap to keyframes").
4. Concat of two same-source clips: **< 5 seconds**, output plays with
   audio in sync.
5. Concat of mismatched-codec clips: **fails with a named reason**, not a
   broken file (LosslessCut's precondition, surfaced).
6. Every render's **exact FFmpeg command is inspectable** (command log).
7. Marks survive asset re-selection within the session (marks are document
   state, not transient button state).

## Improvement targets (where we can beat the references)

1. **Agent-driven editing.** LosslessCut's UI is the only entry point; ours
   composes with the steer conversation — "trim the intro off this and
   concat it with the other clip" dispatches the same governed tools with
   the same provenance. No reference tool has a natural-language EDL.
2. **Agent as the selection engine over a transcript EDL (Reduct's paradigm,
   taken further).** Reduct still requires a human to read the transcript
   and make selections. Our stack composes `transcribe` + the conversation
   so the agent performs the search/selection step: "find the part where he
   graphs Cinderella and make it a clip" — semantic selection, then the
   same `video_clip`/`video_concat` renders. Reduct's interaction model
   with the reading automated.
3. **Subtractive editing as a first-class agent operation.** "Cut all the
   filler/tangents from this lecture" = transcribe → classify unwanted
   passages → render the complement → concat. The classification step is
   what agents are good at and what no reference tool automates.
4. **Provenance on every artifact.** Trim/concat results carry
   tool+args+span provenance (display_hint) — LosslessCut's command log is
   retroactive; ours is structural.
5. **Server-side EDL persistence for free.** `workflow_save`/`workflow_load`
   already exist in the media server — an EDL (segment list or transcript
   selection list) is a workflow document; the references had to build
   project files from scratch.

## Deliberately out of scope (with the reference that draws the line)

- Multitrack, transitions, compositing — MLT's own docs mark the
  single-track playlist as the right scope for simple jobs.
- Destructive editing — GES/MLT model: sources are never modified.
- Re-encode pipelines as the default path — LosslessCut's whole value
  proposition is that they aren't.

## Sources

- LosslessCut README (fetched 2026-08-29):
  https://github.com/mifi/lossless-cut
- MLT Framework Design (fetched 2026-08-29):
  https://www.mltframework.org/docs/framework/
- GES index (fetched 2026-08-29):
  https://gstreamer.freedesktop.org/documentation/ges/
- Reduct.video product site (fetched 2026-08-29):
  https://reduct.video
- Avidemux: http://avidemux.sourceforge.net/
- Our `video_clip` stream-copy implementation:
  `kask/mcp-servers/hkask-mcp-media/src/video/ffmpeg.rs` (`-c copy`,
  `-avoid_negative_ts make_zero`)