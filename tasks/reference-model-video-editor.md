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
2. **Provenance on every artifact.** Trim/concat results carry
   tool+args+span provenance (display_hint) — LosslessCut's command log is
   retroactive; ours is structural.
3. **Server-side EDL persistence for free.** `workflow_save`/`workflow_load`
   already exist in the media server — an EDL is a workflow document; the
   references had to build project files from scratch.

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
- Avidemux: http://avidemux.sourceforge.net/
- Our `video_clip` stream-copy implementation:
  `kask/mcp-servers/hkask-mcp-media/src/video/ffmpeg.rs` (`-c copy`,
  `-avoid_negative_ts make_zero`)