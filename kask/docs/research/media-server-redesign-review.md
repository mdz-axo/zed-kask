---
title: "Media MCP Server — From-Scratch Design Review (Plan and Coverage)"
audience: [operators, developers, architects]
date: 2026-10-06
status: "Active"
kind: research
related:
  - media-server-redesign-findings.md
  - media-server-redesign-proposals.md
  - media-server-redesign-improvement-plan.md
  - media-server-lead-onboarding.md
---

# Media MCP Server — From-Scratch Design Review: Plan and Coverage

**Scope:** `kask/mcp-servers/hkask-mcp-media` (102 registered tools as of
2026-10-06 — the fleet's largest server), its zed-side consumers (`media_panel`,
`hkask-media-widget`), its inference seam (`hkask-inference` media router),
and every dependent skill (discovered empirically, below).
**License:** breaking changes permitted behind the D-seams (D35 media server,
D18 viz widget, D28 artifact storage); every proposal enumerates the path it
replaces and what it deletes. **Deliverables are documents first** — nothing
implements without an accepted slice.

**State:** **All phases (0–6) complete 2026-10-06.** The five operator-named
target conditions were executed as their own kata loops (MF-1…MF-5 —
closure records with receipts in the findings doc); the review phases ran
to complete the doc set: Phase 1 primitives (39 primitives, 8 families,
reconciled to the 102-tool pin), Phase 2 contract-level fidelity (findings
MF-6…MF-8), Phase 3 integration (8 candidates, 1 proposal PR-S1), Phase 4
formal layer (the interpolation contract sweep delivered; Lean deferred
with trigger), Phase 5 proposals (PR-M1 executed, PR-S1 filed for operator
review), Phase 6 plan (the improvement plan doc). The scenarios precedent
(`scenario-server-redesign-*.md`) is the structural model throughout.

## Method

Each phase runs as one PDCA improvement-kata loop (hypothesis FINER-gated →
current condition grounded at file:line → target condition → experiment →
held/refuted verdict). Three goal tags apply throughout: **fidelity** (to the
verified reference models), **surface-efficiency** (the tool surface and its
seams), and **integration** (with the other servers, skills, and the panel).

### Reference models (verified, not presumed)

| Model | Verification | Role in the review |
|---|---|---|
| MovieLabs Ontology for Media Creation (OMC) 2.8 | `onto_anchor` `omc:CreativeWork` → **domain_supplement**, pinned artifact `omc.ttl` (MovieLabs/OMC@dd047298c7aaebf42b71f78f5208a8d872c182ca); vocabulary bridge at `hkask-bridge-ontology/src/omc.rs` | The **creation-vocabulary source**: every tool maps to an OMC concept (`omc::tool_to_omc`), the widget shares the concept→explain dispatch, and `all_terms_are_official` fails the build on invented terms |
| Reduct.video transcript-linked-media discipline | `onto_anchor` `transcript_linked_media` → **derived**, operator ruling 2026-09-25; Reduct.video product and API | The **educt layer system's source**: word-aligned transcript + media as one linked artifact; correcting text never moves timings; reels cut from transcript ranges |
| Reduct.video v3 API reference (operator-supplied) | Pinned provider contract behind `src/tools/reduct.rs` — fixture-tested request path/body, parsed acknowledgement, HTTP refusal | The **cloud write surface's contract**: the 17 `reduct_*` tools implement against it; the unimplemented capabilities (target condition 1) are judged against its pages |
| Ratified contracts in the crate README | `kask/mcp-servers/hkask-mcp-media/README.md` — canonical asset storage (`persist_slim_and_enrich`, one SQLite transaction), per-variant error classification, publication-never-by-default, unencrypted gallery DB | The **server's own ratified contracts** — fidelity means conforming to these, and changing them is a proposal with deletions |

No unregistered-but-real reference model surfaced in Phase 0 (the scenarios
F5/PR-05 class): both ontology-tier models resolve, and the two contract-tier
models are pinned artifacts rather than ontology candidates. If Phase 1
derivation surfaces one, it is recorded as a finding and an operator ruling
is requested — never a fabricated anchor.

### Phase hypotheses (FINER-gated, one PDCA cycle each)

| Phase | Hypothesis | F | I | N | E | R | Verdict |
|---|---|---|---|---|---|---|---|
| 0 Inventory | The tool surface, cross-server consumers, and dependent skills are exhaustively enumerable from source registration + grep, and the recorded ground truth is re-verifiable after the 2026-10-05 commits | 10 | 10 | 8 | 10 | 10 | **Held** — 98 tools reconciled by enumeration = pin = green tests; 6 skills + 1 template found by grep; baseline re-measured 436/0/6 |
| 1 Primitives | A primitive model derived from the four reference models predicts the server's shape with named gaps | 9 | 10 | 7 | 10 | 10 | **Held** — 39 primitives across 8 families, reconciled to the 102-tool pin via `lisp_eval`; every named gap decision-recorded (findings doc Phase 1) |
| 2 Fidelity | Each tool's deviation from the primitive model is evidenced at file:line | 8 | 10 | 6 | 10 | 10 | **Held** at contract level (scope honestly recorded) — 3 findings MF-6…MF-8, all cited; per-family faithful lists in the findings doc |
| 3 Integration | Each candidate integration passes or fails the essentialist Exist/Surface/Contract gates | 8 | 10 | 7 | 10 | 10 | **Held** — 8 candidates: 7 already-exists/no-action, 1 proposal (PR-S1) |
| 4 Formal | The EDL Keep-range union semantics and layer-validation invariants are provable in core Lean under stated assumptions, where property tests do not already pin them against two oracles | 9 | 9 | 7 | 10 | 10 | **Held** — every candidate named with its pins; the interpolation contract gained its second oracle (the 864-case sweep); Lean deferred with trigger, not forced |
| 5 Proposals | The proposal set covers every finding with required fields (id, component, goal tags, evidence, BREAKING CHANGES, effort, depends) | 9 | 10 | 7 | 10 | 10 | **Held** — PR-M1 (executed) + PR-S1 (operator review); map reconciled green |
| 6 Plan | The proposals sequence into dependency-ordered, independently verifiable slices | 9 | 10 | 7 | 10 | 10 | **Held** — 9 slices with receipts and status in the improvement plan doc |

## Phase 0 — Coverage table

Live surface: **98 tools**, pinned by
`tool_surface_is_exactly_98_registered_tools` via the shared
`hkask_mcp_server::tool_surface_pin!` macro
(`src/hkask_mcp_media.rs:486-491`, unified fleet-wide in `ede1ba0ca8`) —
the macro pins both the count and the generated `TOOL_NAMES` set against the
live router. Re-verified 2026-10-06 by enumeration: 98 `#[tool(`-annotated
functions across the ten `src/tools/*.rs` modules, zero elsewhere
(`embed_text` at `src/hkask_mcp_media.rs:409` is an internal helper on the
semantic-search path, not a registered tool). Enumerated 98 = pinned 98 =
green pin test.

### Server tools (98) → module, handler, phase of scope

| Module | Tools (handler line in `src/tools/<module>.rs`) | Phase |
|---|---|---|
| `gallery.rs` (26) | `gallery_organize` L159, `gallery_status` L228, `gallery_search` L250, `gallery_refresh` L542, `describe_image` L675, `gallery_analyze` L715, `gallery_name_face` L804, `face_validate` L871, `face_register` L900, `face_scan_folder` L941, `face_list` L976, `face_remove` L997, `gallery_timeline` L1016, `gallery_record_generation` L1118, `gallery_lineage` L1166, `gallery_list_assets` L1191, `gallery_asset_detail` L1240, `gallery_reproduce` L1306, `gallery_delete_image` L1375, `gallery_add_media` L1449, `gallery_create_album` L1544, `gallery_list_albums` L1569, `gallery_move_to_album` L1584, `gallery_remove_from_album` L1611, `gallery_delete_album` L1639, `gallery_list_album_members` L1657 | 2; face tools also condition 3; `gallery_search` semantic mode also condition 4 (embeds via `embed_text`) |
| `processing.rs` (15) | `image_remove_background` L240, `image_apply_style` L280, `image_create_collage` L329, `video_clip` L529, `video_to_gif` L586, `image_to_video` L660, `video_add_caption` L709, `video_remix` L767, `video_from_images` L860, `video_concat` L936, `video_caption` L987, `video_extract_frames` L1070, `video_meme` L1182, `video_info` L1282, `video_fetch` L1308 | 2; `video_fetch` also condition 5 (DNS-rebinding preflight) |
| `educt.rs` (15) | `educt_store_transcript` L288, `educt_list_transcripts` L323, `educt_get_transcript` L351, `educt_delete_transcript` L392, `educt_store_layer` L426, `educt_list_layers` L453, `educt_paragraph_pass` L472, `educt_speaker_pass` L528, `educt_correction_pass` L614, `educt_apply_corrections` L670, `educt_highlight_pass` L750, `educt_edl_from_highlights` L813, `educt_render_edl` L919, `educt_export` L1049, `educt_locate` L1212 | 1, 2, 4; correction/export/locate also condition 2 (re-alignment) |
| `reduct.rs` (17) | `reduct_connection_status` L1066, `reduct_connection_probe` L1076, `reduct_projects_snapshot` L1086, `reduct_recordings_snapshot` L1099, `reduct_reels_snapshot` L1120, `reduct_reel_detail` L1136, `reduct_create_reel` L1152, `reduct_add_reel_clip` L1173, `reduct_add_reel_title` L1203, `reduct_edit_reel_clip_range` L1231, `reduct_create_recording` L1259, `reduct_import_media` L1274, `reduct_upload_gallery_media` L1353, `reduct_upload_local_media` L1371, `reduct_recording_highlights` L1402, `reduct_recording_status` L1418, `reduct_recording_transcript` L1434 | 1, 2; write surface also condition 1 |
| `audio.rs` (8) | `voice_design` L165, `generate_speech` L221, `transcribe_bundle` L269, `transcribe_and_store` L308, `audio_capture` L362, `record_and_transcribe` L403, `audio_trim` L503, `audio_concat` L557 | 1, 2; transcription tools also condition 2 (re-alignment source) |
| `generation.rs` (6) | `generate_image` L11, `transform_image` L170, `upscale_image` L226, `generate_video` L255, `expand_prompt` L306, `image_edit_region` L372 | 2 |
| `jobs.rs` (4) | `job_submit` L99, `job_list` L336, `job_status` L378, `job_cancel` L414 | 2 |
| `workflows.rs` (4) | `workflow_save` L19, `workflow_list` L53, `workflow_load` L82, `workflow_delete` L102 | 2 |
| `models.rs` (2) | `model_list` L125, `model_info` L145 | 2 |
| `youtube.rs` (1) | `youtube_search` L167 | 2 |

### Cross-server surface

| Consumer | Location | Relationship |
|---|---|---|
| `media_panel` | `crates/media_panel/src/media_panel.rs` | Re-exports `TOOL_NAMES`; renders the Steer prompt's grouped tool advertisement (`hkask_steer::render_grouped_tool_advertisement`) — the name pin is that chain's integrity check |
| `hkask-media-widget` (D18) | `crates/hkask-media-widget/src/{hkask_media_widget,media_widget}.rs` | Renders ```media display-hint blocks; shares the OMC concept→explain dispatch with the server (single implementation in `hkask-bridge-ontology/src/omc.rs`) |
| `hkask-inference` media router (D35) | `hkask-inference/src/{media_router,media_providers}.rs` | The two inference routes (vision/generation) every media tool dispatches through; routing policy is an operator decision recorded in its README |
| `kask_bridge` registration | `kask_bridge/src/mcp_servers.rs` | Built-in server `id: "media"`, binary `hkask-mcp-media`, env allowlists |
| `hkask-mcp-server` pin macro | `hkask-mcp-server/src/hkask_mcp_server.rs` | Shared `tool_surface_pin!` + build-support scan (single implementation, `ede1ba0ca8`) |
| corpus server | `kask/mcp-servers/hkask-mcp-corpus/` | **Composition-level consumer only**: `educt_export` `corpus_text` output feeds corpus ingestion via the `transcript-reel` skill; grep found no direct code reference (the apparent `educt` hits in corpus sources are `reduction_pct` false positives) |
| spreadsheet what-if | `hkask-mcp-companies` spreadsheet tools; `hkask-spreadsheet` | **Parallel display-hint contract family**: ```media blocks (media server, `src/media_block.rs`) and ```spreadsheet blocks (companies server) are both panel-rendered viz widgets (D18) — a contract-shape sibling, not a code consumer |

### Dependent skills (grep-verified over `.agents/skills/**` and `kask/registry/templates/**`)

| Skill | Relationship | Evidence |
|---|---|---|
| `transcript-reel` | Primary educt consumer — 14 tools: `record_and_transcribe`, `transcribe`, `transcribe_and_store`, `transcribe_bundle`, `educt_store_transcript`, `educt_correction_pass`, `educt_apply_corrections`, `educt_speaker_pass`, `educt_paragraph_pass`, `educt_highlight_pass`, `educt_edl_from_highlights`, `educt_render_edl`, `educt_export`, `educt_locate` | `SKILL.md`; reifies the `transcript_linked_media` discipline (ruling 2026-09-25) |
| `media-workflow` | Primary gallery/generation consumer — 17 tools across its fixed pipelines | `SKILL.md` |
| `listening` | Consumer — `transcribe` (MAIA listening template), `educt_locate` (citation verification) | `SKILL.md` |
| `grounding-verify` | Consumer — `educt_locate` (quote location), `model_inference` | `SKILL.md` |
| `sankey-flow` | Consumer — `transcribe` | `SKILL.md` |
| `docproc/ocr-extract.j2` | Template consumer — media tool references in a registry template | `kask/registry/templates/docproc/ocr-extract.j2` |

**Coverage claim:** every one of the 98 tools appears in Phase 2's scope
(all modules), with the condition-linked tools additionally scoped to their
target condition's kata loop; every cross-server consumer and all 6 skills
appear above. The review is complete when read with the findings document
and, as Phases 5–6 run, the proposals and improvement plan.

## Ground truth (MOVE 0 record, 2026-10-06)

The onboarding record's verification (2026-09-30, baseline 432/0/6) predates
the 2026-10-05 media commits; everything below was re-measured this session.

**Test baseline (this session's, not the record's):** `cargo test -p
hkask-mcp-media` → **436 passed, 0 failed, 6 ignored** (310 main binary +
125 request-deser totality + 1 doc-test). The 6 ignored are the opt-in live
Reduct probes (`HKASK_REDUCT_LIVE_PROBE=1` + configured OS keychain
required); they fail without the env var, as designed. The +4 delta over the
record's 432 is the 2026-10-05 commits' tests (classification pins, batch
return-type adaptation).

**Tool-surface pin:** enumerated 98 `#[tool(`] fns = pinned 98 = green.
The pin mechanism changed under the server on 2026-10-05: the per-server
test is now one `tool_surface_pin!` macro call
(`src/hkask_mcp_media.rs:486-491`) pinning count + generated name set
(`TOOL_NAMES`, built by `build.rs` from the `#[tool]` fn scan) against the
live router.

**The 2026-10-05 commits (what changed since the record):**

| Commit | Change |
|---|---|
| `ede1ba0ca8` | Pin unification: shared `tool_surface_pin!` macro + build-support scan across all servers |
| `078fade04a` | Missing caller-named resources → `not_found` across servers |
| `d17d9e7a92` | Media errors per canonical contract: `ImageNotFound` → `not_found`, `GalleryNotInitialized` → `failed_precondition` (both were `invalid_argument`), pinned in tests |
| `afc97e99a9` | Media `InferencePort` test stubs single-sourced behind one macro |
| `988eb466c3` | Usage/cost propagated through embed/rerank |
| `c657ee8d30` | Test fixup (unused binding) |
| `8a3b973f39` | Media embed adapted to batch return type |

**The five target conditions (onboarding record §6), re-grasped at current
file:line** — full records in `media-server-redesign-findings.md`:

1. **Reduct cloud write surface** — strikethroughs, highlight writes,
   redactions, publishing, media download, transcript correction
   unimplemented (`README.md:340-344`); Page 41 of the v3 reference lists
   strikethrough paths without body/response schema; no private Reel
   render/download contract. Never publish by default (`README.md:338`).
2. **Transcript re-alignment** — cardinality-changing corrections leave text
   readable/corpus-exportable but timed consumers fail with an explicit
   unaligned precondition (`README.md:252-259`); local
   re-transcription/re-alignment is the honest capability gap.
3. **Face-recognition build-out** — explicitly deferred; the two vision-LLM
   templates (`validate_face_ref`, `match_faces` in `src/templates.rs`)
   are the working core (`README.md:379-383`). **Operator decision**
   required.
4. **Embedding-error typing** — `classify_embedding_error` string-matches
   credential-missing substrings (`src/error.rs:181-184, 220-227`) because
   `EmbeddingGenerationError` has no typed `NotConfigured` variant
   (`hkask-types/src/ports/embedding.rs:30-42`); the doc comment at
   `src/error.rs:174-181` names the migration path. Note: `d17d9e7a92`
   (2026-10-05) fixed the adjacent `ImageNotFound`/`GalleryNotInitialized`
   classifications — a different slice of the error contract, not this one.
5. **DNS-rebinding limitation** — the widget preflight cannot pin opaque
   yt-dlp/FFmpeg connect-time DNS; documented as a transport limitation,
   not a claimed guarantee (`README.md:83-85`). **Operator decision**
   required (accepted-risk vs. mitigation).

## Execution report — ran vs. proposed

**Ran (oracle-verified receipts in this session):**

- Test baseline re-measured: 436/0/6 (counts per binary recorded above).
- Tool-surface enumeration: 98 `#[tool(`] fns counted per module
  (8+15+26+6+4+2+15+17+4+1), zero outside `src/tools/`; reconciled against
  the 98 pin and the green test run.
- Reference-model registration verified: `onto_anchor` on
  `omc:CreativeWork` (domain_supplement, pinned OMC 2.8 artifact) and
  `transcript_linked_media` (derived, ruling 2026-09-25).
- Cross-server consumers and dependent skills enumerated by grep (tables
  above), including the negative result: the corpus server is a
  composition-level consumer only, and the apparent code hits were false
  positives.
- The five target conditions re-grasped at current file:line (citations
  above; full records in the findings doc).
- Calibration prediction recorded before the work item:
  `kanban_goal_create` goal `e42b567d-11f3-47e3-b6e4-5b7f6dad4221`,
  prediction 0.85 — the operator's `kanban_goal_score` is the only Brier
  signal; this report is the measurement the score closes over.

**Proposed, not run (by design — deliverables are documents until a slice
is accepted):**

- Phases 1–6 of the review (primitives → fidelity → integration → formal
  → proposals → plan). The findings doc opens Phase 1.
- The five target conditions' kata loops. Two are operator decisions
  (face-recognition build-out; DNS-rebinding ruling) — presented in
  functional terms with costs when their loops run, never decided here.
- Implementation of anything: nothing implemented, nothing committed this
  session.

**Skill battery:** loaded and followed — `program-manager` (the governing
rubric), `kata-improvement` (the per-phase PDCA loop), `hypothesis-framer`
(FINER gating). Applied from principle without a load (context budget):
`metacognition` (predict → measure → operator scores),
`pragmatic-semantics` (IS/OUGHT on findings), `essentialist`
(Exist/Surface/Contract in Phase 3), `deep-module` (deletion test on
proposals), `grill-me` (decoupled verify of each phase deliverable),
`pragmatic-cybernetics` (loop health on touched feedback signals),
`lean-prover` (Phase 4), `diagnose` (any bug the work surfaces),
`media-workflow` (verification pipelines), `improv` (operator posture).
