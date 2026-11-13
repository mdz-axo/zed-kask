---
title: "media panel — composition & interaction review (S6/S7 axis): findings, receipts, and the S6 design pre-review (2026-11-06)"
audience: [developers, architects]
last_updated: 2026-11-05
version: "1.0.0"
status: "Active"
domain: "Media panel / UI"
mds_categories: [composition, domain]
---

# media panel — composition & interaction review (S6/S7 axis): findings, receipts, and the S6 design pre-review (2026-11-06)

Convergent code review of the composition and interaction patterns across the
reel-composer surface and the S6/S7 arcs, run per the composition-review
continuation prompt (composed at `c10f39bb1d`). Method: the `code-review`
skill's ordering (machine checks first, multi-axis detection, IS/OUGHT
adjudication, falsifiers, file:line no-fiction, decoupling disclosure), with
`refactor-architecture`/`deep-module` as inline lenses (delegation folded
into the axes) and `kata-improvement` as the routing target for accepted
findings. The stated specs: plan §4 S5/S6/S7 acceptance
(`kask/docs/plans/media-panel-transcript-bundle-plan-2026-11-04.md` L274-362),
reference.md v3.4.0's documented contracts, the server's correction/export
types, and W3C REC 2017-02-23 / MDN WebVTT karaoke / the OTIO schema for S7.

**Decoupling: `fresh_session`.** This session authored none of the reviewed
code; the handoff prompt was its only input. The S5/S7 author's own
disclosures were treated as claims to verify, not trusted.

**Prediction (pre-registered before code reading):** ~10 findings, 0
blockers, on the basis that both landed slices were green-gated with
red-first documentation and the acceptance criteria were met. Found: 14
findings, 1 waivable Blocker (doc rot, fixed in-review). Gap [+4, +1],
reconciled below — the Blocker is citation rot, not code: the landed code
held; the author-disclosed citation-refresh risk was worse than disclosed.

## 1. Machine-check receipts (live, at review HEAD; no inherited green)

Review HEAD at pickup: `73ada7a3fc` (three commits past compose-time
`c10f39bb1d`, all other streams). Working tree clean at pickup.

| Gate | Receipt |
| --- | --- |
| `cargo test -p media_panel` | `test result: ok. 126 passed; 0 failed` |
| `cargo test -p hkask-mcp-media` | `test result: ok. 162 passed; 0 failed` + 1 doctest ok |
| `./script/clippy -p media_panel` | exit 0 (script splices `--deny warnings`) |
| `./script/clippy -p hkask-mcp-media` | exit 0 |

S7's fixture pins in the server suite: `webvtt_cues_carry_karaoke_tags_and_dot_millis`,
`webvtt_without_word_timings_is_a_named_degradation`,
`web_annotation_carries_both_selectors_and_motivations`,
`otio_reel_clips_carry_schema_keys_and_source_ranges`
(`kask/mcp-servers/hkask-mcp-media/src/transcript_export.rs:838,851,870,966`)
— all byte-for-byte value equality, not structural spot-checks.

CI at pickup: run `42170125609` (docs-head `a8426d25e7`) failed on the
**duplication ratchet: `FAIL: 5608 duplication clusters exceed the baseline
5599`** — a tree-wide live red the P2-stream commit `631d4468b2` discloses
in its own message (5613 at its compose). Not a media-arc failure;
consequence recorded under finding F15: S6/S7 landings inherit the red
ratchet and must adjudicate their own duplication (baseline bump in the
earning commit).

## 2. Scope adjudication (state at pickup)

- **S6: NOT landed** — board task `5e871bd4` backlog, unclaimed. Reviewed as
  pre-implementation DESIGN (findings-on-paper).
- **S5: landed** (`76b6b99907`, +1411/-231 over `media_viewer.rs` +
  reference.md) — reviewed as CODE.
- **S7 server half: landed** (`83d122809c`, +650/-26) plus the
  `631d4468b2` sweep-in of the `layer_id: None` test-call-site fix
  (+12, disclosed in that commit's message) — reviewed as CODE.
- **S7 panel half: NOT landed** — task `e0c1cf12` in_progress; no
  `crates/media_panel` commits since `76b6b99907`; the compose-time dirty
  files were the test call sites, now committed under the P2 message.
- The handoff's line anchors re-verified **exact** at pickup HEAD (no
  drift): `reel_selection:880`, `compose_workspace_reel:3949`,
  `render_workspace_reel_section:7836`, `ReelComposeMode:9379`,
  `union_word_ranges:9427`, `compose_reel_params:9880`,
  `selection_tray_mode:9091`, `dispatch_workspace_correction:3588`,
  `apply_workspace_correction_result:3774`, `popover_correct:4485`,
  `render_selection_popover:4958`.
- Plan §4's own anchors (`begin_edit ...:1250`, `transcript_alignment_note:8425`,
  `compose_reel_params:9351`, `union_word_ranges:8898`) are point-in-time —
  the plan is historical intent, not a living contract; their drift is
  expected and is NOT the reference.md rot class (finding F5).

## 3. Findings

Severity derives from constraint force (CR-1 receipts retained in the
session log; the fixed order base → confidence → provenance-ceiling →
taste-ceiling applied). "Fixed in-review" findings carry their fix commit.

### Blocker (1 — fixed in-review; open set now empty)

**F5 — reference.md v3.4.0's pre-S5 sections carried 61 rotted citation
anchors.** [CODE · Guardrail (the `.rules` diff-driven citation rule) ·
conf 1.0 · IS · direct_measurement]
S5's reference refresh (+107/-66) updated the sections it authored — the
S2/S5/popover/cuts/reels sections verify exact today (`selection_tray_mode:9091`,
`popover_cut:4535`, `popover_include_in_reel:4587`, `open_label_editor:4225`,
`compose_reel_params:9880`, `union_word_ranges:9427` all match the tree) —
but the earlier sections' anchors stayed at pre-S5 positions: 61 anchors
drifted, e.g. `start_transcript_follow` doc'd `:4689` (actual `:5077`),
`transcript_search_matches` `:9456`→`:9985`, `transcript_alignment_note`
`:8084`→`:8818`, `render_transcript_workspace` `:6306`→`:6694`, the
Request-ownership table's `apply_*` rows (+17 through the request/apply
region), and the `verified_against` header's `RequestOwner :99-109` →
`:107-117`. This is the 2026-11-01 lesson's second observed specimen
(refresh applied to the pattern-matched sections, not the diff-driven
file). The reference is the landed-contract doc — its citations are
load-bearing for exactly this kind of review.
**Falsifier:** the extraction ladder (§6) re-run against the pre-fix doc
produced 50 stale pairs; against the fixed doc it reports only 8 pairs,
each adjudicated as a pairing artifact (multi-symbol rows pairing the
neighboring anchor — e.g. `apply_detail_result` paired with
`load_detail`'s `:1902`). **Fix:** commit `d2b65de490` (+68/-64, includes
the version bump to v3.4.1) — 61 anchor substitutions, each verified
against the tree; two of the first-pass `sed` line addresses were no-ops
(`:4689` sits on the line before the assumed one), caught and completed
by the post-edit ladder re-run.

### Should-fix (8)

**F1 — S6's spec does not name the membership model it replaces.**
[DESIGN · Guideline (P5.5) · conf 0.85 · OUGHT]
Plan §4 S6 "Replaces/deletes" names only the unordered
`union_word_ranges` composition (exactly one call site:
`media_viewer.rs:9913` inside `compose_reel_params`). The landed
membership model is `reel_selection: HashSet<(usize, usize)>`
(`media_viewer.rs:880`) with three write paths: the popover toggle
(`popover_include_in_reel`, `:4587-4614`), the highlight-row toggle
(`:7689-7694`), and the reload rebuild (`apply_transcript_result:3149-3154`).
S6's design puts the include flag inside `ClipSpec` — if `reel_selection`
survives beside the ordered list, the pane holds a set-and-list pair that
can drift. **Falsifier:** the S6 diff deletes `reel_selection` and its
three write paths (or documents one surviving source of truth and why).
**Remedy:** name the membership model in S6's Replaces/deletes before the
slice is claimed.

**F2 — reload semantics wipe composer deltas mid-compose.** [DESIGN ·
Guideline · conf 0.8 · IS (landed behavior) over OUGHT (S6 must account
for it)]
Every annotation CRUD dispatch ends in `load_transcript_with` →
`apply_transcript_result`, which rebuilds `reel_selection` from ALL
researcher highlights (`media_viewer.rs:3149-3154` — "a fresh workspace
loads with ALL researcher-authored pane selections included"). Under S6,
any mid-compose strike/correction/highlight reloads the workspace and
silently re-includes every excluded clip — and, unless the design
addresses it, resets order and strikes. The plan's S6 section does not
name reload persistence. **Falsifier:** an S6 interaction test that
orders/excludes/strikes, performs an unrelated annotation edit, and
asserts composer state survives the reload. **Remedy:** key ClipSpec
state to highlight identity (annotation id) across reloads, or snapshot
and re-match composer state on reload — and extend the teardown-symmetry
suite (S1-S3 pattern) to cover it.

**F3 — in-reel strike must bake into Keep ops; the natural helper is
private.** [DESIGN · Guideline (D5 no-fork) · conf 0.9 · IS over OUGHT]
The server's selection algebra unions ALL Cut ops globally and subtracts
them from EVERY Keep range (`transcript_select.rs:231-254`) — so a
per-clip strike CANNOT be expressed as a Cut op in the composer EDL (it
would cut every clip overlapping that word range). The plan's "per-clip
difference math" (splitting the one clip's Keep op) is the correct
reduction and the store-time validation
(`transcript_layers.rs:322` → `edl_to_keep_ranges` on store) accepts the
split form. But the exact subtraction helper,
`subtract_cuts_from_range` (`transcript_select.rs:311`), is private — S6
will fork the difference math unless it is published. **Falsifier:** the
S6 op builder produces only Keep ops for strikes (no Cut ops in a
composer EDL), and the difference math lives in one home shared with the
algebra. **Remedy:** publish `subtract_cuts_from_range` (or add a pub
per-clip difference helper beside the algebra) and have the panel builder
call it — the S5 precedent (`merge_over_prior` reuse, `media_viewer.rs:3694`)
is the standard.

**F4 — `union_word_ranges` mirrors the server's pub `union_ranges`.**
[CODE · Guideline (D5) · conf 0.95 · IS]
`media_viewer.rs:9427-9442` re-implements `transcript_select.rs:263-276`
(sort + merge overlapping AND adjacent) over tuples; the module is `pub`
(`hkask_mcp_media.rs:227`) and the panel already links the server crate
and reuses its functions directly (`merge_over_prior`, `CorrectionEdit`
at `media_viewer.rs:3692-3697`). The mirroring is documented
(`:9423-9426`) but a second home for the composition algebra can drift.
One call site (`:9913`). **Falsifier:** the S6 ordered-builder change
deletes `union_word_ranges` and calls the server's `union_ranges` where
union is still needed. **Remedy:** ride S6 (P5.5 — same change).

**F6 — reference.md's tray-mode sentence over-claimed.** [CODE ·
Guideline (doc truth) · conf 0.9 · IS] — **Fixed in-review** (`d2b65de490`).
"The index-identical-only actions (Highlight, Cut, Include in reel) refuse
on re_anchored views even when reached directly" was false as written: the
highlight ROW's Include toggle (`media_viewer.rs:7689-7694`) mutates
membership on any aligned view without refusal — and that is principled
(it flips stored source-anchored membership, never a working selection;
the popover's refusal at `:4591` guards the working-index create arm).
The sentence now scopes the refusal to the selection entries and states
the row toggle's source-anchored safety. **Falsifier:** the row path
anchors working indices — it does not: `popover_include_in_reel` reads
the highlight's stored `start_word/end_word` (`:4604-4608`).

**F7 — `educt_export`'s tool description predates the three new formats.**
[CODE · Guideline (contract-surface completeness) · conf 0.85 · IS]
`tools/educt.rs:1960` names SRT/corpus_text/review_csv anchoring but not
webvtt (timing-aligned WORKING view, `:2167-2168`), web_annotation
(immutable SOURCE words — the W3C brittleness-note anchoring,
`:2198-2199` over `bundle.words`), or otio (EDL layer resolution
`:2237-2266`). The anchoring asymmetry (web_annotation=source,
webvtt=working) is exactly what the primary contract surface should
disclose; the param doc (`types.rs:347-360`) does name all formats.
**Falsifier:** an agent reading only the tool description infers the
anchor state of each format correctly. **Routed:** S7 card `e0c1cf12`.

**F8 — the OTIO layer-resolution branches are unpinned.** [CODE ·
Guideline (error-contract pins) · conf 0.95 · IS]
The Otio arm's resolution has five branches — named-id found
(`:2238`), named-id absent (not_found naming the id, `:2245-2248`),
named-id wrong kind (invalid_argument naming the kind, `:2255-2262`),
latest-EDL (`:2239-2242`), and none (not_found "compose a reel first",
`:2249-2253`) — plus the empty-keeps refusal (`:2263-2267`). The S7
tool-level tests cover srt/highlights_csv/corpus_text/review_csv/reject-
unknown-format; the new formats carry only serializer fixtures and the
request-deser totality. Grep over `hkask_mcp_media.rs` for
`webvtt|web_annotation|otio` returns zero. The S7 panel half will consume
exactly these branches. **Falsifier:** tool-behavior tests in the
`educt_export_highlights_csv_without_layers_is_not_found` style cover
each resolution arm. **Routed:** S7 card `e0c1cf12`.

**F10 — the preview's word→time source must match the render path's.**
[DESIGN · Guideline (preview/render consistency) · conf 0.75 · IS over
OUGHT]
The render path maps clip word ranges to times through the stored source
bundle's `TimedWord`s (`edl_to_clip_plan` → `word_range_to_time_range`;
the OTIO arm passes `&bundle.words`, `:2269`). Clip ranges are SOURCE
indices. On a `re_anchored` working view the working words' timings are
interpolated estimates over DIFFERENT indices — a preview that reads the
working view's word timings (as the follow-along does,
`media_viewer.rs:5106-5110`) will disagree with the rendered reel on
corrected views. The plan's S6 design ("auto-seek chain on the shared
player — the follow-loop's task pattern") does not name the timing
source. **Falsifier:** an S6 preview test over a `re_anchored` fixture
asserts each preview seek matches `edl_to_clip_plan`'s output for the
same clip. **Remedy:** preview maps source ranges through the same
bundle-words path the render uses (or names the divergence honestly).

### Nit (1)

**F11 — the dispatch-chain prologue is duplicated five times.**
[CODE · Preference (taste) · conf 0.8 · IS]
`create_workspace_highlight:3504`, `create_workspace_strike:3833`,
`delete_workspace_annotation:4051`, `compose_workspace_reel:3949`, and
`dispatch_workspace_correction:3588` share a ~12-line prologue (state
check → transcript_id → `begin_edit` match → progress →
invoker-unwired-with-per-flow-failure). The full chain is NOT worth
extracting: the apply-result stage carries per-flow status text and
result types, and a generic chain abstraction is the trait-with-one-impl
trap. A shallow `begin_workspace_edit` helper is defensible when S6 adds
its compose dispatches. Named here as the decoupling disclosure, not as
a defect. **Falsifier:** S6's dispatches read end-to-end without
scrolling through five near-identical prologues.

### FYI (4)

- **F9 — preview-lifecycle substrate (verified facts for S6's design):**
  the follow-loop pattern to reuse is `start_transcript_follow:5077-5110`
  (single-instance guard, GPUI-native timer, exit-on-surface-change at
  `:5086-5091`, poll-only-while-playing, position through
  `transport_position_secs` — the S4 rate-conversion accessor); the
  teardown sites to join are `:1084` (surface switch), `:2469` (open),
  `:2541` (close) plus the reload paths (F2); hidden media clears the
  widget (`suspend_media_widget:1001` → widget `suspend`) — the preview
  loop must exit on widget-None; and the 82.7-minute seek-jump
  performance concern is measured-first per the plan (hold S6 to it).
- **F13 — the author's S5 disclosures verify:** all four
  `render_label_editor` match arms carry `.to_string()` on the context
  string (`media_viewer.rs:4801-4812`); every `TranscriptPaneState`
  literal carries `correction_edits` (mechanically: the crate compiles —
  25 `Vec::new()` + 1 shorthand + 2 `..state` update sites; 32 total
  mentions); the S5-section citations in reference.md were exact (the
  drift was elsewhere — F5).
- **F14 — the S5 server-contract checks all held:** the strict parse's
  silent-revert guard rejects a named-but-absent correction layer from
  the same payload's layer list (`media_viewer.rs:10475-10483`); the
  locate disambiguation is deterministic
  (`parse_correction_source_range:9187-9219` — the candidate whose working
  range exactly matches); the trap pin asserts the TRANSLATED source
  range (`:14779-14782`: `edits[1]["start_word"] == 40`, "the new edit
  anchors the SOURCE range") with supersession surfaced (`:14796`:
  "superseded 1"); the merge reuses the server's `merge_over_prior`
  (`:3694`); the tray-mode gate covers the popover's direct-reach paths
  (`popover_cut:4539`, `popover_include_in_reel:4591`) and the reload
  clears stale editors/popovers (`:3155-3159`); the blank-replacement
  refusal is panel-side with its pin (`:4289-4304`, pin `:12507`).
- **F15 — CI context:** the failing run is the duplication ratchet
  (5608 > 5599) on a docs-head commit; the P2 stream carries the debt
  disclosure; S6/S7 landings inherit the red ratchet (each arc's
  committed duplication is its own debt — bump the baseline in the
  earning commit) and S6 is the plan's only L-slice (jscpd report names
  resolve by unique suffix, never repo-root).

## 4. The eight axes — composition-pattern analysis

1. **One composition model, not two.** The landed model is genuinely one
   (the HashSet, rebuilt from highlights on reload — F2's semantics), but
   S6's ClipSpec list must REPLACE it, not join it (F1). The single
   `union_word_ranges` call site makes the P5.5 replacement small and
   clean.
2. **The strike math must not fork the algebra.** The global-cut-union
   fact (`transcript_select.rs:231-254`) forces the bake-into-keeps
   design; the private `subtract_cuts_from_range` is the fork risk (F3);
   `union_word_ranges` is an existing fork with one call site (F4). The
   S5 precedent (`merge_over_prior` reuse) is the standard S6 should
   meet.
3. **Provenance identity holds under the ordered form.** Compose
   provenance is `pane_reel_compose` (`media_viewer.rs:9934`), pinned
   never to match the strike identity (test comment `:14969`; pin
   `compose_reel_params_builds_each_mode` `:14870`). The strike parse
   requires user + researcher_annotation + exactly one Cut op — an
   ordered multi-Keep composer EDL cannot collide by construction. S6's
   acceptance-3 extension should pin the ordered form AND a
   strike-carrying composer EDL (bake-into-keeps means no Cut ops — pin
   that invariant itself).
4. **Preview lifecycle.** The substrate facts are F9; the open design
   questions are F2 (reload) and F10 (timing source). The search-focus
   suspension precedent (`ensure_transcript_search_focus_listeners:4762`
   — suspend on focus, restore only if the search took it) is the
   symmetry model for preview-vs-follow: preview should suspend
   viewport tracking only while it drives seeks, and restore the prior
   state on exit.
5. **The dispatch-chain family.** F11: keep local; the family grows with
   S6 but the per-flow apply-result stage blocks a clean generic
   extraction. The honest duplication is the prologue; a shallow helper
   at S6-landing time is the remedy if the repetition bothers then.
6. **The S5 landed surface.** All six checks held (F14); the two
   author-disclosed landing risks verify clean (F13); the one true
   defect the S5 landing introduced was the partial citation refresh
   (F5, fixed in-review).
7. **The S7 serializer discipline.** Met: pure functions over stored
   layers (`transcript_export.rs:301,359,421,521`), deterministic
   byte-for-byte fixture pins, no Python anywhere in the S7 surface,
   Web Annotation carries BOTH selectors with motivations
   `highlighting`/`tagging` and creator `researcher_supplied`
   (`:465-491`, W3C REC 2017-02-23 forms), WebVTT cues parse with
   karaoke tags (`:359-384`, MDN form), OTIO clip keys match the
   documented schema with the round-trip deferral STATED in the test
   doc (`:962-964`), and exports land outside the playable gallery
   (`publish_document` → `generated_assets_dir()/transcript-exports`,
   `:140`) with the staged-rename-verify discipline (`:226-241`). The
   anchoring choice — position AND quote selectors over the immutable
   SOURCE words (`word_char_offsets:389`, `web_annotation_collection`
   fed `&bundle.words` at `tools/educt.rs:2199`) — matches the study §4
   brittleness note exactly; the Web Annotation test pins computed
   character offsets (`"beta gamma" spans chars 6..16`, `:868`).
   Open items: F7 (description), F8 (resolution pins). The panel-half
   design answer: exports dispatch read-only — they should ride the
   `detail_request`-style read lifecycle (`media_viewer.rs:1022-1026`),
   NOT the edit admission; the preview is player-local and needs no
   admission (the existing seek paths are the precedent).
8. **Admission vs read lifecycles.** The three shapes are clean in the
   landed panel: mutating dispatches gate on `begin_edit` (`:1267-1274`),
   reads gate on the `detail_*` request lifecycles, and player-local
   actions (seeks, `popover_copy_time`'s clipboard write) take no gate.
   S6's preview classifies player-local; S6's compose dispatch stays
   admission-gated (two-invoke chain under one admission — the landed
   `compose_workspace_reel` pattern); S7's export dispatches classify
   read-lifecycle. A correction blocked behind an export read would be
   the violation to watch for when the panel half lands.

## 5. S6 design pre-review (findings-on-paper, DESIGN vs CODE)

Tagged DESIGN (changes the spec before implementation — cheapest to fix
now) or CODE (binds the implementation, not the plan text):

| # | Finding | Tag | Severity |
| --- | --- | --- | --- |
| F1 | Name `reel_selection` + its three write paths in Replaces/deletes | DESIGN | Should-fix |
| F2 | Reload-rebuild wipes composer deltas; spec must address persistence | DESIGN | Should-fix |
| F3 | Bake strikes into Keep ops; publish/share the subtraction helper | DESIGN + CODE | Should-fix |
| F10 | Preview timing source must match the render path's bundle words | DESIGN | Should-fix |
| F4 | Delete the `union_word_ranges` mirror riding the ordered builder | CODE | Should-fix |
| F9 | Preview substrate checklist (follow-loop pattern, teardown sites, suspension, measure-first) | CODE | FYI |

The S6 acceptance criteria (plan L312-320) otherwise hold as written:
acceptance-1's strike-difference boundary cases come free if F3's
shared-helper remedy is taken (the algebra's own tests carry the
boundary semantics); acceptance-2's scripted-widget preview test should
add the F2 reload case and the F10 re_anchored fixture case; acceptance-4
(the Dunning end-to-end) is the operator-verifiable deliverable and
should exercise one mid-compose annotation edit (the F2 falsifier) as
part of its run.

## 6. Re-runnable checks

- Machine gates: `cargo test -p media_panel` (expect 126), `cargo test -p
  hkask-mcp-media` (expect 162), and the clippy gate once per crate
  (`./script/clippy -p media_panel && ./script/clippy -p hkask-mcp-media`,
  expect exit 0 each).
- Citation ladder (the F5 fix's oracle; run from the repo root):

```bash
doc=kask/docs/diataxis/media_panel/reference.md
src=crates/media_panel/src
rg -oU --multiline-dotall '`([a-zA-Z_][a-zA-Z0-9_]*)`[^`]{0,90}?:([0-9]+)' \
  -r '$1 $2' "$doc" | sort -u | while read -r sym anchor; do
  actual=$(rg -n "fn ${sym}\b|struct ${sym}\b|enum ${sym}\b|const ${sym}\b|^    ${sym}:|^    ${sym}\b" \
    "$src/media_viewer.rs" "$src/media_panel.rs" 2>/dev/null \
    | grep -v '^[^:]*:[0-9]*: *//' | head -1 | cut -d: -f2)
  [ -n "$actual" ] && [ "$actual" != "$anchor" ] && echo "STALE $sym $anchor -> $actual"
done
```

Expected output at `d2b65de490`: exactly the 8 adjudicated pairing
artifacts (`apply_detail_result 1902`, `apply_gallery_listing 1448`,
`apply_jobs_listing 1618`, `apply_live_observation 1754`,
`apply_transcript_result 2460`, `schedule_queue_poll 1727` —
multi-symbol rows pairing the neighboring anchor; `jobs_restart_behavior
6160` — resolves to the field decl at :753 while the citation targets the
drawer-footer render site at :6160; `QUEUE_POLL_INTERVAL 55` —
GALLERY_PAGE_LIMIT's anchor). Any OTHER line is new rot — a line-shifting
edit to `media_viewer.rs` re-runs this ladder in the same change.

## 7. AI-code annex

All reviewed code is AI-authored; the annex applied to every change.
Invented APIs: none found (every symbol cited resolves — verified by the
ladder's definition lookups and by compile). Stray imports: none
observed. Hardcoded values duplicating config: `MAX_CUE_WORDS=15`,
`ANNOTATION_CONTEXT_CHARS=32`, `ANNOTATION_CREATOR` are documented
serializer constants, not config drift. Out-of-scope edits: the
`631d4468b2` sweep-in of 12 test-call-site lines under an unrelated
commit message is disclosed IN that message (the shared-index hazard,
handled honestly); no other out-of-scope edits found in the reviewed
commits. LLM-loop gaming: not applicable (no scorer in scope). The
one AI-authored defect class this review found in the reviewed arcs is
partial-refresh drift (F5) — an LLM-authored refresh pattern-matched the
sections it knew it had touched instead of diff-driving the file.

## 8. Coverage honesty

**Checked:** the S5 dispatch/correction chain end-to-end
(`dispatch_workspace_correction:3588-3767`,
`apply_workspace_correction_result:3774-3805`, the parse
`:10462-10526`, the pins at `:14691-14800`, `:12507`, `:14520-14573`);
the compose path (`compose_workspace_reel:3949-4043`,
`compose_reel_params:9880-9940`, `union_word_ranges:9427-9442`,
`ReelComposeMode:9379-9421`, the reel section `:7836-7882`, the
membership paths `:4601-4614`, `:7663-7694`, `:3149-3154`); the server
algebra (`transcript_select.rs:185-330`), store-time validation
(`transcript_layers.rs:322`), the S7 serializers
(`transcript_export.rs:270-597`), the export tool arms
(`tools/educt.rs:1960-2299`), `publish_document` (`:133-270`), and the
fixture pins (`:838-1010`); the tray-mode gate and its direct-reach
guards (`:9091-9103`, `:4539-4544`, `:4591-4599`); the read lifecycles
(`:1013-1031`); the follow-loop substrate (`:5077-5110`); every citation
anchor in reference.md (the ladder).
**Not checked:** the Dunning end-to-end round trip (needs S6 — the asset
and transcript exist per the handoff but were not exercised); the S7
panel half (not landed); the media widget beyond the S4 accessor
contract (out of scope); the OTIO round-trip fidelity (honestly deferred
per the plan — structural keys only); runtime behavior of the preview
(does not exist yet); the 82.7-minute seek performance (no S6 to
measure).
**Residual risk:** the S6 design findings (F1, F2, F3, F10) are
paper-level — their falsifiers bind the implementation, not this review;
the S7 panel half may introduce new lifecycle findings when it lands;
the ladder's pairing heuristic has the 8 documented artifacts and cannot
verify anchors without a resolvable symbol antecedent (prose anchors like
the former `:5634` were verified manually).

## 9. Routing and landed work

- **Landed (this review):** `d2b65de490` — reference.md v3.4.1, the F5
  citation refresh (61 anchors, each re-verified) + the F6 sentence
  scoping + the version bump. Net +68/-64; replaces the stale anchors of
  v3.4.0. No other files touched; no gates affected (doc-only; the
  citation ladder is the oracle and it re-runs clean).
- **Routed to S6 (board task `5e871bd4`, backlog):** F1, F2, F3, F10 (design
  input before claim), F4 (rides the ordered-builder change), F9
  (substrate checklist). Posted as a card comment for the claiming
  stream; the operator adjudicates at the algedonic review — the report
  proposes, the board carries what is accepted.
- **Routed to S7 (board task `e0c1cf12`, in progress):** F7 (tool
  description) and F8 (OTIO resolution pins) — both in the S7 stream's
  write scope, posted as a card comment.
- **No goal operations** — the goal is closed; findings route to the
  board and the operator only.
- **`.rules` proposal:** none new — F5 is the `.rules` citation-rot rule's
  second observed specimen, which STRENGTHENS the existing entry rather
  than needing a new one. The candidate addition (declined as
  non-essential): "the partial-refresh failure mode recurs when the
  refresh is pattern-driven; the diff-driven sweep is the whole file's
  citation set" — this is already the existing rule's content.

## 10. Loop closure

**Verdict: Approve** (CR-2: open_blockers 0, health improves — the landed
S5/S7-server surface is green-gated, acceptance-met, and its one
contract-doc defect is fixed in-review; the S6 design findings route as
pre-implementation input, not as defects of landed work).

**Prediction reconciliation:** predicted 10/0, found 14/1 (+4/+1). The
under-prediction is calibration data: the author-disclosed citation risk
was materially worse than the disclosure suggested (partial refresh, 61
anchors), and the design-axis work produced four paper findings the
prediction under-counted.

**lessons_learned:** (1) A reference-doc citation refresh must be
diff-driven over the whole file — the second observed partial-refresh
specimen (S5) matched the first (2026-11-01) in mechanism: refresh what
you know you touched, miss what you shifted. The ladder (§6) makes the
whole-file sweep cheap; it should re-run with every line-shifting change
to the cited file. (2) `sed` line-addressed batches need per-line
post-verification even when all commands are substitutions — two of 61
were silent no-ops because the anchor sat on the adjacent line; the
post-edit ladder re-run caught both (the `.rules` multi-sed lesson,
verified live again). (3) The global-cut-union fact of the server's EDL
algebra is load-bearing for any per-clip strike feature — it lives in
`edl_to_keep_ranges`'s body, not its signature, so it must be read, not
inferred.

**next_review_focus:** the S6 landing review should walk, in order: the
Replaces/deletes diff (F1 — the membership model's tombstone), the
reload-persistence interaction test (F2's falsifier), the op-builder's
strike output (F3 — keeps only, shared subtraction home), the
preview/render timing-source equality over a `re_anchored` fixture
(F10), the extended provenance pin (acceptance-3), and the duplication
ratchet delta (S6 is the plan's only L-slice). The S7 panel-half review
should check the export dispatches ride the read lifecycle (axis 8) and
the OTIO resolution pins (F8) exist before the panel lands on those
branches.
