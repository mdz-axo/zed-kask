---
title: "Media panel navigation redesign — plan (two task modes + activity layer)"
audience: [developers extending the media panel, operators]
last_updated: 2026-11-02
version: "1.0.0"
status: "Active"
domain: "Media panel / UI navigation"
mds_categories: [domain, composition, lifecycle]
---

# Media panel navigation redesign — plan

**Execution state (recorded 2026-11-13):** In execution — operator-ruled 2026-11-02; A–C landed (19ce009a62 asset workspace, 1644a9e20f activity layer, 5006241eba transcript relocation); D (docs, steer text, and removal accounting) in flight.

Date: 2026-11-02 · Author: Z-K agent (technical program manager) · Operator:
principal investigator — ruled the model 2026-11-02 ("please proceed as
proposed"). Basis: completed research run `cd195abc1a7889e3` (8 sources,
6 domains, no duplication, avg confidence 0.85, sensitivity stable) +
working-tree code anchors verified this session. Claim tags: **[run]**
server-recorded source in the research ledger · **[tree]** file:line
verified in the tree · **[ruling]** operator decision.

## 0. Done means (functional outcome)

- The media panel's primary navigation mirrors user tasks: exactly two
  task modes (Library; Asset workspace) plus one ambient activity layer.
  The five subsystem-shaped tabs (Media, Library, Queue, Detail,
  Transcript) are gone.
- You select an asset and stay with it: the player, the edit toolbar,
  and a toggleable inspector pane (record, lineage, tags, notes,
  transcripts) are one screen; transcripts open on the asset.
- Job state reaches you where you are (badges on affected assets, an
  on-demand drawer you can cancel from), never as a place you must
  visit.
- The governing principle is pinned in the panel reference so no future
  panel re-introduces subsystem-shaped navigation.

## 1. The governing principle (pinned)

**Primary navigation mirrors user tasks, never server subsystems.**

What it forbids: top-level destinations that are 1:1 mirrors of server
tool families (an index list, a job ledger, an inspector payload, a
transcript store) — the five-tab model, where each tab fronts exactly
one MCP tool family and the panel's own steering text describes tabs in
server terms ("the Library, Queue, and Detail tabs expose indexed
assets, research jobs, and provenance", media_panel.rs:273-277 [tree]).

What it requires: modes are verbs a person performs on their material
(browse; work on one asset); inspection is a supporting pane of the
object being worked on; process state is ambient status. Tabs remain
legitimate only as alternate views of one object inside a pane.

Published anchors [run]: Apple HIG "Split views" — the inspector pane
surrounds the canvas (Keynote example); Apple HIG "Tab views" — tabs
present "closely related areas of content"; Apple HIG "Progress
indicators" — progress is transient, present only while an operation is
ongoing; Android canonical layouts — list-detail and supporting pane
("a supporting pane tool window is irrelevant by itself"); Final Cut
Pro — background tasks are automatic, observed through an on-demand
window (Window > Background Tasks), never a peer destination;
Lightroom — modules are workflow stages; grid/loupe are view modes
within a module.

## 2. The ruling [ruling]

Operator confirmed 2026-11-02 on the three questions raised:

1. Queue is not its own tab — it is process state (`sumo:Process`,
   "anything that occurs in time but [is] not an Object"); its
   user-facing form is ambient status plus an on-demand monitor. It is
   not "part of the library" either: jobs without a gallery subject
   render only in the activity layer.
2. Media and Detail are one workspace — the viewer is primary, the
   inspector is its supporting pane.
3. The five tabs conflated code categories for user modes — confirmed
   against the tree [tree]: Media fronts the player plus sync edit
   tools (`dispatch_edit`, media_viewer.rs:966); Library fronts
   `gallery_list_assets`; Queue fronts `job_list`/`job_cancel`/
   `job_submit` (:4479); Detail fronts `gallery_asset_detail` (:1521);
   Transcript fronts the `educt_*` family (:6365).

Functional criteria (the model's observable conditions):

1. Exactly two task modes plus an ambient activity layer; the five-tab
   model is gone.
2. Job state is ambient — badges on affected assets and a cancel drawer;
   no job surface is a peer navigation destination.
3. The selected asset's record, lineage, tags, notes, and transcript
   links render as a toggleable inspector pane inside the asset
   workspace, with pane-internal tabs.
4. Transcript browsing, reading, highlight selection, and reel
   composition live on the Library and asset-workspace surfaces; no
   standalone transcript destination remains.

## 3. Phases (the board's tasks)

Serialized over `crates/media_panel/src/media_viewer.rs` — the phases
share one file, so the dependency chain is real contention, not a false
dependency. Board: "Media panel redesign".

| # | Task | Est | Depends on |
|---|---|---|---|
| T1 | Asset workspace: merge Media and Detail into one mode with a toggleable inspector pane | 6h | — |
| T2 | Activity layer: replace the Queue tab with job badges, a status strip, and an on-demand cancel drawer | 6h | T1 |
| T3 | Transcript relocation: recordings browser into the Library, transcript workspace onto the asset | 6h | T2 |
| T4 | Docs, steer text, and removal accounting | 4h | T1–T3 |

Per-task acceptance criteria live on the board cards (the durable
ledger): the §2 criteria specialized per phase, plus the verification
floor — `./script/clippy` over the touched scope and
`cargo test -p media_panel` with pasted output — and the P5.5 rule: a
phase that only adds is not done; each phase names what it replaces and
deletes (old render paths, purpose lines, tests of removed behavior)
and reports net lines.

## 4. Migration sketch [tree]

| Current surface | Anchor | Becomes |
|---|---|---|
| `ViewerTab` enum (5 variants) | media_viewer.rs:528 | 2 modes + drawer state |
| `render_media` (player, marks, trim, concat) | :4090 | Asset workspace primary area |
| `render_detail` (7 inspector sections) | :4615 | Toggleable inspector pane, pane-internal tabs |
| `render_queue` (rows + cancel) | :4479 | Activity drawer rows + asset badges |
| `apply_jobs_listing` (1 Hz poll + result ingest) | :1380 | Unchanged engine |
| `render_transcript_browser` (recordings list) | :6097 | Library section/filter |
| `render_transcript` workspace | :4883 | Asset-workspace sub-surface |
| `transcribe_recording` (job_submit op=transcribe) | :6365 | Unchanged; badge added |
| `dispatch_edit` (sync edits) | :966 | Unchanged |
| Purpose lines (`tab_purpose`) | :4289, :4467, :4593, :4629, :6128 | Rewritten for modes/pane/drawer |
| Panel doc comment + steer prompt | media_panel.rs:5-6, :203-278 | Rewritten (T4) |

## 5. Recomposition and final verification

- Strategy: serialized phases, each landing green (clippy + tests +
  moved pins) before the next begins; the final tree is the composition.
- Integration order: T1 → T2 → T3 → T4.
- Final verification: full `cargo test -p media_panel`; `./script/clippy`
  over the touched scope; operator walkthrough — select an asset,
  inspect its tabs, trim to marks, transcribe from a library row, watch
  the badge, open the drawer, cancel a job, open the transcript, select
  a highlight, compose a reel.

## 6. Carried-forward rulings and honest wrinkles [ruling] [tree]

- Trim serves audio and video through one position source (fc4f469dc5)
  — the workspace keeps this; no per-kind split.
- The purpose-line orientation contract (7651569521) survives: every
  mode still opens with one line of purpose copy; the five lines are
  rewritten for the new model.
- The researcher annotation editor stays on the inspector (2026-10-27).
- R5 poll-to-refresh (a landed transcribe appears under its recording)
  and the R3 recording linkage are preserved by T3.
- Wrinkles: unlinked transcripts (recordings removed from the index) get
  a Library section with honest degradation copy; agent-submitted jobs
  without a gallery subject render only in the activity drawer; the
  process-local history disclosure (ephemeral_process_local restart
  behavior, jobs.rs:9-12) moves to the drawer.
- The loop-register L11 deferred impedance (an older running job behind
  the 20-row page loses automatic observation, loop-register.md:521-524)
  is settled by T2's drawer contract, not carried forward silently.
- Keymap check [tree]: the panel registers only `media_panel::Toggle`
  and `ToggleFocus` (media_panel.rs:27-36); no keymap binding references
  media-panel actions (assets/keymaps/*.json carry no media-panel
  entries), so no keymap sweep is required by this redesign.

## 7. References [run]

Verified full-text (6): Apple HIG — Split views; Progress indicators;
Tab views. Apple Support — View background tasks in Final Cut Pro for
Mac. The Lens Lounge — Allan (2022), "Adobe Lightroom library module
explained". Android Developers (2026) — Canonical layouts. Inferred
(2): Pond5 — "Getting Started With Final Cut Pro X: An Interface
Overview"; Sunwall (2024) — "Tabs, Used Right" (NN/G, cited via
syndication listing; the nngroup.com/articles/tabs/ URL 404'd — both
states recorded in the run ledger). Evidence set: 8 sources, 6 domains,
no duplication, avg confidence 0.85, sensitivity stable.