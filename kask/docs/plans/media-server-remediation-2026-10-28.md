---
title: "Media MCP server remediation plan — refactor-architecture run"
audience: [architects, developers, operators]
last_updated: 2026-10-28
version: "1.0.0"
status: "Proposed — dependency-ordered remediation plan for operator review; not yet executed"
domain: "Media / hkask-mcp-media"
mds_categories: [domain, lifecycle]
---

# Media MCP server remediation plan — refactor-architecture run (2026-10-28)

**Commission:** operator commission — run `refactor-architecture` over
`kask/mcp-servers/hkask-mcp-media/` to remediate accumulated breakage and
feature drift, delivering a dependency-ordered remediation plan.

**Method:** the skill's PDCA loop with a bounded evidence-first sweep of
four classes (broken behaviors, additive drift, interaction-pattern
drift, orphaned surface), findings at file:line, execution of the
contract-repair tier under shared-tree discipline, and the deferred tiers
gated on the in-flight work.

**Aperture:** the media MCP **server** (`kask/mcp-servers/hkask-mcp-media/`).
The panel-side run (follow-along scrolling, reel affordances, recordings
tab) is a sibling plan committed as
`kask/docs/plans/media-refactor-2026-10-28.md`; the two compose without
overlapping edits.

**Tree state:** sweep at HEAD `161ccfe035`; repairs landed at
`8656895e17` and `2df4b64986` (all local on `main`).

**RA-2 receipt:** `target_condition_status: "met"` for the executed tier,
deepening checklist `unmeasured` (deferred per EUREKA-2 D8) → `partial`.

## 1. Reference model (verbatim)

Source: `~/Documents/zk-data/zk-reference-open/reference-docs/reduct-transcript-model.md`
(zk-ref-open; CC BY 4.0), reified in-repo by the `transcript-reel` skill
(operator ruling 2026-09-25) with MovieLabs OMC provenance:

> Transcript-linked media couples a time-aligned transcript with its
> source recording so that text and media address each other. The
> reference model: (1) the transcript is the index — every word (or
> doc-range) carries a time span, so any text selection maps to media
> time and any media span maps to text; (2) a reel is an ordered
> composition of doc-range clips — the reel references transcript spans,
> never re-encodes media, so a reel is cheap, reversible, and always in
> sync with the transcript; (3) recordings are project-scoped assets
> that media (video/audio) uploads attach to, and transcription status
> is a per-recording state; (4) highlights are labeled selections — the
> semantic layer that reels are composed from. The discipline the model
> teaches: edit at the reference layer (spans, orders, labels), render
> only on demand — never mutate media to edit meaning.

## 2. Findings (all reproduced at HEAD 161ccfe035 unless noted)

### Class 1 — broken behaviors

| ID | Severity | Finding |
|----|----------|---------|
| B1 | HIGH | `range_extraction_gates_on_start_and_keeps_source_pts` fails 0-vs-1: the fake ffmpeg captured the argument after `-vf` (the select filter) as its output path, writing a file named after the filter into the crate root and leaving the real pattern file unwritten (`src/video/ffmpeg.rs:1128-1137`; command shape at `:733-741`, `:901-917`) |
| B2 | HIGH | `layers_semantic_pass_shot_mode_...` self-deadlocks: it calls `media_test_env()` while `vision_pass_env` already holds `ARTIFACTS_ENV_LOCK` (a non-reentrant global tokio mutex) — the hold stalls every env-lock test, the recorded full-suite timeout (`src/hkask_mcp_media.rs:7236-7242`); it also stored the shot layer in a second store the semantic server cannot resolve (`tools/layers.rs:682` looks up by its own driver) |
| B3 | HIGH | `tool_surface_is_exactly_81_registered_tools` fails: router registers 82 (`src/hkask_mcp_media.rs:466-471`) |
| B4 | HIGH | `omc_mapping_covers_all_registered_tools` fails: no `voice_identify` arm in `src/omc.rs` |
| B5 | HIGH | `live_router_schema_inventory_is_complete_and_clean` fails: pins 81 while its own arithmetic message computes 82 (`tests/schema_compliance.rs:54-60`) |
| B6 | HIGH | `dual_axis_covers_all_registered_tools` fails: no `voice_identify` arm in `src/ontology.rs` |
| B7 | HIGH | panel pin fails: `TOOL_NAMES.len() == 81` (`crates/media_panel/src/media_panel.rs:481`) |
| B8 | MED | voice tools map encoder failures blanket `unavailable` and transcript-store failures blanket `internal`, bypassing the canonical per-variant mappers (`tools/voice.rs` at the 2026-10-28 tree; the ruling is documented at `error.rs:103-112`) |
| B9 | LOW | `let _ = summary;` lint-suppression hack in `voice_identify` |

B3–B7 are one root cause: commit `161ccfe035` added `voice_identify`
without its registration. Six CI gates fail at once — the
tool-registration-totality lesson (curator memory, 2026-10-13)
recurred fifteen days after capture.

### Class 2 — additive feature drift

- **D1 (MED, deferred):** `video_caption` runs its own
  frame-sampling → read → single-vision-call orchestration parallel to
  the `layers_semantic_pass` chunk core (`tools/processing.rs:738-835`
  vs `tools/layers.rs:736+` + `layer_pass.rs`). The layer-pass core is
  the newer, more disciplined path (bounded chunks, resume cursor,
  plan-vs-observed coverage, typed errors); the older tool was never
  migrated. Deletion test: the transient-caption operation earns its
  keep — the duplicated *mechanics* are the deepening candidate.
- **D3 (falsified — not drift):** the `edl` and `audio_event` layer
  kinds have no producer/consumer today, but the EUREKA-2 plan
  (`kask/docs/plans/eureka2-media-educt-cycle-plan-2026-10-28.md` §2)
  declares them the planned t5/t8/t9 surface. The 2026-10-13 verdict
  "additive drift, all new tools anchored" holds for the current wave:
  the transcript-linked semantics are aligned; what was left unrepaired
  was the verification surface, not the semantics.

### Class 3 — interaction-pattern drift

- **I1 (MED, PI decision):** `educt_delete_transcript`
  (`tools/educt.rs:1003-1032`) removes the transcript and its layers
  atomically, but voice-profile rows keep referencing it:
  `authorization_transcript_id` (`voice_profiles.rs:224-234`) and
  segment `transcript_id` (`:239-244`) dangle, un-surfaced in
  `voice_list`. Same family, reverse direction: layers validate
  `gallery_asset_id` only at store time. Options (functional, PI's):
  (a) surface the dangle in `voice_list` provenance; (b) block deletion
  while live profiles reference the transcript; (c) cascade-delete
  derived profile segments. Server recommendation: (a) — honest,
  cheapest, no destructive surprise.
- **I2 (verified aligned):** the transcript-linked core discipline
  holds across the wave — word-indexed layers validated against word
  count (`educt_store_layer`), immutable sources with supersession
  surfaced (`transcript_retranscribe.rs`), EDL composition from spans
  rendered on demand with lineage and intermediate cleanup
  (`educt_render_edl`), delete preserving exports/renders with detached
  identities surfaced in the response.

### Class 4 — orphaned surface

- **O1 (HIGH, fixed):** the 5-byte file
  `select='isnan(prev_selected_t)*gte(t,10.000)+gte(t-prev_selected_t,1)',showinfo,scale=640:-1`
  in the crate root — B1's side effect, committed in `6436fcb964`.
- **O2 (LOW):** stale doc counts — `kask/docs/reference/mcp-servers/README.md`
  pinned "media 80" while the surface reached 82; the onboarding and
  media reference docs' educt counts (21 → 22).
- **O3 (work record):** the t4 card sat in backlog with its code
  landed and a stale build-blocker note; card comments at HEAD now
  record the resolution.

## 3. Task graph (dependency-ordered)

Ordering rule: contract repair → drift removal → structural deepening.

| Task | Resolves | What it fixes / replaces / deletes | Deps | Verification oracle | Risk out of order |
|------|----------|-------------------------------------|------|---------------------|-------------------|
| T1 Repair the shot-mode test | B2 | one env, one store; delete the second `media_test_env` block | none | `cargo test -p hkask-mcp-media layers_semantic_pass` (was: infinite hang) | without it the whole suite cannot run (env-lock deadlock) |
| T2 Repair the range test + remove the stray file | B1, O1 | fake writes to the command's LAST argument; delete the filter-named file | none | `cargo test -p hkask-mcp-media range_extraction` (was: 0-vs-1 fail) | independent |
| T3 Registration totality | B3–B7, O2 | pin 81→82 + test rename; `voice_identify` arms in omc.rs and ontology.rs; schema inventory count + arithmetic; panel pin; README count | none | `tool_surface_is_exactly_82…`, `omc_mapping_covers…`, `dual_axis_covers…`, schema_compliance, panel `steer_prompt…` | must land as ONE change — the pin surfaces move together |
| T4 Voice error-mapping alignment | B8, B9 | `map_voice_encoder_error` (Media → `map_media_error`, Download → `unavailable`, Io/Integrity/Runtime → `internal`); seven store calls → `map_store_error`; delete the `let _ = summary` hack; new classification pin test | T1 (suite green to verify) | voice set + full suite + clippy | low |
| T5 Flaky EDL render test stabilization | known weak spot (plan Phase 0, `.rules`) | **EXECUTED — see §4, `76df402d6a`**: root cause was the shared-root observation race, not the render | T1 | the three rescoped tests + full suite green at the touched tree | the standing isolated-re-run rule is retired |
| T6 Cross-store cascade decision (I1) | I1 | **EXECUTED — see §4, `b261a0fc0d`**: the platform's own precedent (the transcript store's detach triggers) settled the policy — detach-and-surface, never block, never cascade-delete | T3 | both new tests + full suite green | vetoable on functional grounds (the PI ratification is the algedonic review's) |
| T7 `video_caption` strangler-fig (D1) | D1 | migrate the transient-caption mechanics onto the layer-pass core; delete the duplicated orchestration; behavior-parity tests before deletion | **post-t10** (EUREKA-2 D8) | parity tests + full suite + net-lines negative | walking the in-flight layer surface mid-design churns t5–t10 |
| T8 Docs count refresh | O2 (rest) | onboarding/reference educt counts; media.md tool reference | with t10 | t10 card criteria | none |

## 4. Execution record (this run)

- `8656895e17` — **Repair the media server verification surface**:
  T1 + T2 + T3 in one domain commit. 8 files, +23/−17 (net negative
  counting the stray-file deletion). Verification: both repaired tests
  pass isolated; `tool_surface`, `dual_axis`, `schema_compliance` green;
  full media suite **407 lib + 161 schema + 1 doctest, 0 failed, 60.79s**
  (previously timed out); panel pin green; `./script/clippy -p
  hkask-mcp-media -p media_panel` exit 0 including machete.
- `2df4b64986` — **Classify voice tool errors per variant**:
  T4. +65/−12. Verification: voice set 16 passed; full suite
  **408 + 161 + 1, 0 failed, 59.36s**; clippy exit 0.
- `76df402d6a` — **Make the render-intermediate assertions hermetic**:
  T5, root-caused. The flake was never the render: the before/after
  snapshot walked the shared `hkask-media` temp root, written by every
  concurrent ffmpeg runner on the machine (sibling tests, live
  servers). The observation is now the server's own runner temp
  directory (`FfmpegRunner::temp_dir()`), which no other runner can
  legally write. +30/−8. Verification: the three rescoped tests pass;
  full suite green at the touched tree.
- `b261a0fc0d` — **Detach and surface cross-store references on
  deletion**: T6, root-caused. Voice profiles (separate DB) gain the
  read-side resolution — `voice_list` reports each profile's
  authorization transcript status (present/deleted); media layers
  (same DB) gain the trigger mirror of the transcript store's asset
  detach — `gallery_asset_id` severed and `detached_at` stamped on
  asset deletion, surfaced in list/get. +253/−9. Verification: both
  new tests pass; full suite **412 lib + 161 schema + 1 doctest,
  0 failed**; clippy exit 0.
- Card comments recorded on t2 (`bcd53d23`), t4 (`169940b9`), and the
  speaker-identity Phase-3-prep card (`5ffe9726`) clearing their recorded
  blockers; the t4 card remains its owner's to verify and move.
- The tool-registration-totality lesson (curator h_mem) updated with the
  recurrence evidence.

## 5. Premise verdict (honest)

The premise — "prior agent work added easy features while leaving
breakage unrepaired" — is **confirmed** in its concentrated form: at the
sweep tree, two defective tests (one deadlocking, one failing with a
repo-polluting side effect) and one five-surface registration omission
left **seven failing gates**, while adjacent streams kept shipping
features (the voice stream even fixed its own hang with a 10s visible
timeout, `1e9761009a`, in the same hours). The additive drift itself is
**aligned** per the reference model: no parallel EDL path, no
transcript-media coupling violation found in the wave; the `edl` /
`audio_event` "dead vocabulary" hypothesis was **falsified** by the
EUREKA-2 plan (planned t5/t8/t9 surface). Scope was shrunk accordingly:
the deepening tier is planned, not executed.

## 6. Proposed `.rules` addition (for the operator)

> Adding an MCP tool to a pinned server updates every pin surface in the
> same change — the lib `tool_surface_pin!` count, the `omc.rs` mapping
> arm, the `ontology.rs` dual-axis arm, the schema-compliance inventory
> arithmetic, and consumer pins (e.g. `media_panel`'s `TOOL_NAMES.len()`)
> all fail CI as a set; the pins catch the omission only after the
> broken commit lands (161ccfe035 → six red gates; repaired 8656895e17).

## 7. Coordination notes

- The `edl`/`audio_event` kinds stay: they are the declared t5/t8/t9
  surface; do not delete them in orphan sweeps.
- The media server's `hkask-storage` dep is live (five production
  modules); the 2026-10-13 "residual dep" note is stale.
- The EDL-render flake is root-caused and fixed (`76df402d6a`): the
  intermediate-observation was a race against every concurrent runner
  on the machine, scoped now to the server's own runner temp directory.
  The standing isolated-re-run rule is retired.