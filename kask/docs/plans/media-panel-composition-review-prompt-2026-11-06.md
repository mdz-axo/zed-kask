---
title: "media panel transcript bundle — composition & interaction review continuation prompt (S6/S7 axis)"
audience: [agents, developers]
last_updated: 2026-11-05
version: "1.0.0"
status: "Active"
domain: "Media panel / UI"
mds_categories: [lifecycle, composition]
---

# media panel transcript bundle — composition & interaction review continuation prompt (S6/S7 axis)

**Handoff target:** any agent with repo access and no conversation history.
**Composed:** 2026-11-06, from the S5 session. Tree verified at HEAD `c10f39bb1d`
(origin/main == HEAD at compose time, nothing unpushed); every fact below was
checked against the tree at compose time — re-verify at pickup, the tree moves
(the S7 stream was still editing `hkask_mcp_media.rs` and `tools/voice.rs` when
this was composed).

**Read first (required, in order):**
1. The worktree-root `.rules` (the only `.rules` that loads).
2. The `code-review` SKILL.md, loaded via the `skill` tool — its method
   (machine checks first, multi-axis detection, the AI-code annex, IS/OUGHT
   adjudication, falsifiers, file:line no-fiction, decoupling disclosure)
   governs this whole run. Then `refactor-architecture`, `deep-module`, and
   `kata-improvement` as routed below.
3. `kask/docs/plans/media-panel-transcript-bundle-plan-2026-11-04.md` §4 — the
   S6 and S7 sections are the stated specs this review adjudicates against;
   §5 is the verification discipline; §3 Phase C names the gates.
4. `kask/docs/research/media-transcript-interaction-study-2026-11-04.md` §4
   (the standards mapping — W3C Web Annotation, WebVTT karaoke, OTIO) and
   §6 P2-6 (the reel-composer grounding: Reduct's verified reel pattern) and
   §7 (the GPUI build discipline).
5. `kask/docs/diataxis/media_panel/reference.md` (v3.4.0) — the landed
   contract. Its "Transcript surface", "Playback and layout contracts", and
   capability-summary sections are the delivered-surface claims to check
   the code against.
6. `kask/docs/research/media-panel-transcript-bundle-goal-closure-verification-2026-11-06.md`
   — the closure verification's evidence discipline (grounding-verify, ISA
   230): the review's receipts should hold to the same bar.
7. The board "Media panel transcript bundle" (`8820ae83-a57f-45d6-9caf-23320e687ca8`);
   the S4 card's mechanism comments (task `ea1aa377` — the rodio Speed
   semantics the preview math must respect) and the S5 card's verify evidence
   (task `45c383ac`).

## Mission

Run the convergent code review of the **composition and interaction
patterns** across the reel-composer surface and the S6/S7 arcs, with the
architecture axis over the composition machinery, and route accepted findings
into the kata improvement loop as board work. The review covers, in priority
order:

1. **The S6 design, pre-implementation** (S6 was unclaimed backlog at compose
   time — the plan's only size-L slice). Findings on paper are the cheapest
   findings this review will produce; S6 lands on whatever patterns survive.
2. **The landed composition/interaction surface** the composer builds on and
   the freshest landed machinery it rides: the pre-existing reel compose
   path (S6's substrate and declared replacement target), the S5 tray and
   dispatch patterns (commit `76b6b99907`), and the S7 server exports
   (commit `83d122809c`).
3. **Whatever landed between compose time and pickup**: S6's implementation
   if it has landed (review it against plan §4 S6's acceptance), and S7's
   panel-half affordances if they have landed (review them against §4 S7's
   acceptance). Verify state first; never review dirty in-flight bytes as if
   they were landed, and never touch another stream's uncommitted files.

S4 (the widget transport speed work) is out of scope except where S6's
preview math reads player positions (the rate-conversion contract).

## Review axes — the concrete pattern questions

These are starting questions from the composing session, not a cage: the
skills' own methodologies govern, and surprises outside these axes are
findings too. The S5 items in axis 6 are the author's own disclosures —
verify, don't trust.

1. **One composition model, not two.** The landed membership model is
   `reel_selection: HashSet<(usize, usize)>` plus the highlights-row
   Include-in-reel toggle; S6 introduces an ordered `ClipSpec` list
   (highlight identity + word range + include flag + in-reel strike
   sub-ranges). The plan is explicit: the unordered `union_word_ranges`
   composition is REPLACED by the ordered builder, with the unordered call
   sites deleted in the same change (P5.5). Adjudicate the design (or the
   landed code, if S6 landed): one source of truth for membership and order,
   or a set-and-list pair that can drift?
2. **The strike math doesn't fork the server's selection algebra.** In-reel
   strike is a per-clip sub-range Cut; the server's `edl_to_keep_ranges`
   (`kask/mcp-servers/hkask-mcp-media/src/transcript_select.rs`) validates
   Keep/Cut semantics at store time. The panel's ClipSpec difference math
   must reduce to ops the server accepts without re-implementing its
   algebra. Same question for the panel's `union_word_ranges` vs the
   server's union semantics: legitimate projection helper or fork? The
   S5 precedent is the standard: `dispatch_workspace_correction` REUSES the
   server's `merge_over_prior` rather than reimplementing supersession.
3. **The provenance identity holds under the ordered form.**
   `pane_reel_compose` provenance must never parse as a researcher strike,
   and the identity pin extends to the ordered op form (plan S6
   acceptance 3). If S6 landed: the extended pin exists and its red run is
   documented. If not: the design names how it will.
4. **Preview lifecycle against the shared-surface patterns.** The preview
   is an auto-seek chain on the shared player (span end → seek next span
   start, the follow-loop's task pattern). Adjudicate its interactions:
   the follow-along during preview seeks (the search-focus suspension
   precedent — does preview suspend/re-enable viewport tracking
   symmetrically?), the visibility-managed suspension (hidden media
   `suspend()`s — what happens to a preview mid-suspension?), teardown
   symmetry (close/open/surface-switch/reload reset preview state — the
   S1–S3 pattern), and the 82.7-minute seek-jump performance concern the
   plan flags (measure before UX polish). Any position math reads
   media-scaled time: rodio reports wall-equivalent positions under
   `Speed` (the S4 card's mechanism note) — `AudioPlayer::position` and
   the widget accessors already scale back; S6's math must go through
   them, never raw wall reads.
5. **The dispatch-chain family: extract or keep local.**
   `create_workspace_highlight` (single invoke), `compose_workspace_reel`
   (store→render), `dispatch_workspace_correction`
   (locate→store→realign→reload) share a
   `begin_edit → invoker → parse → apply_workspace_*_result → reload`
   shape. Adjudicate Ousterhout-style: is a shared chain abstraction a
   deep module waiting, or does extraction trade end-to-end readability
   for speculative generality (the trait-with-one-impl trap)? Name the
   duplication honestly either way (the decoupling disclosure), and let
   the answer cover S6's compose and S7's export dispatches too.
6. **The S5 landed surface (commit `76b6b99907`) — the freshest specimen,
   reviewed against plan §4 S5 and the server contracts:**
   - The strict parse of the live correction layer's edits (the
     silent-revert guard): a `correction_layer_id` naming an absent layer
     record rejects the payload; check the server guarantees the named
     layer is in the same list it was named from.
   - The locate disambiguation (`parse_correction_source_range`): the
     candidate whose working range matches the selection — deterministic,
     never a guess; the stored edit anchors the TRANSLATED source range
     (the `.rules` retranscribe trap — the pin exists; check the pin
     actually asserts the translated range, not the working range).
   - The merge reuses the server's `merge_over_prior` — no fork; the
     supersession count surfaces in the status, never silent.
   - The tray-mode gate (`selection_tray_mode`): Full on original/aligned,
     CorrectOnly on re_anchored, visible refusal on unaligned/untimed; the
     index-identical-only actions (Highlight/Cut/Include) refuse on
     re_anchored views even when reached directly. S6's composer entry
     points must be gated consistently — check the design.
   - The blank-replacement refusal (deletions belong to Cut) is
     panel-side: the server's store validation checks bounds and
     disjointness only — verify the panel guard is the only thing
     standing between the editor and an empty-replacement layer, and
     that the pin covers it.
   - Author-disclosed landing risks to verify (do not trust): the
     `render_label_editor` match arms carry `.to_string()` on every arm's
     first element (two fuzzy-edit near-misses during landing were caught
     by the compiler and a git-diff audit — verify the landed bytes);
     every `TranscriptPaneState` literal carries `correction_edits`
     (a scripted 23-site field addition — grep-count them against the
     struct); the reference.md v3.4.0 citation refresh (spot-check five
     citations against the tree).
7. **The S7 serializer discipline (server half landed in `83d122809c`).**
   Pure functions over stored layers; deterministic, fixture-pinned
   byte-for-byte; NO Python anywhere (`.rules`); the Web Annotation
   entries carry TextPositionSelector + TextQuoteSelector, motivations
   `highlighting`/`tagging`, creator `researcher_supplied` (W3C REC
   2017-02-23); WebVTT cues parse with karaoke tags (MDN); OTIO clip
   keys match the schema (full round-trip honestly deferred to
   operator-side tooling — the deferral is stated, not smuggled).
   Adjudicate the anchoring: position selectors anchor the immutable
   source word indices; quote selectors carry text — decide which view's
   text (source or corrected working) and check the choice against the
   study §4 brittleness note. Exports land OUTSIDE the playable gallery
   (the durable-export pattern). If the panel affordances have landed:
   they dispatch read-only — adjudicate whether they take the edit
   admission (over-serializing: a correction blocked behind an export?)
   or a read lifecycle (the `detail_request` pattern). The preview is
   player-local and needs no admission.
8. **Admission vs read lifecycles, consistently.** The workspace now has
   three dispatch shapes: mutating (admission-gated: annotations,
   corrections, compose), read-only server reads (lifecycle-gated:
   detail, transcripts, exports), and player-local actions (preview,
   seeks — no gate). Every S6/S7 entry point should classify cleanly.
   A read hiding behind the edit admission, or a mutation riding a read
   lifecycle, is a finding.

## Skills routing (the method)

1. **`code-review`** — the primary instrument. Load it first; its
   ordering governs: machine checks FIRST when runnable, then multi-axis
   detection, the AI-code annex (all of this work is AI-authored — the
   annex applies to every change reviewed), IS/OUGHT adjudication,
   falsifiers per finding, file:line no-fiction, decoupling disclosure.
   The stated specs: plan §4 S5/S6/S7 acceptance criteria, reference.md
   v3.4.0's documented contracts, the server's correction/export types,
   W3C REC 2017-02-23 / MDN WebVTT / the OTIO schema for S7.
2. **`refactor-architecture`** — the composition axis: friction discovery,
   deepening candidates, the design-tree walk, the duplication audit
   (the axes above feed it), strangler-fig migration planning for S6's
   unordered→ordered replacement.
3. **`deep-module`** — the deletion test, interface minimalism, and
   dependency direction over the modules the arcs created or will create
   (the correction chain, the export serializers, S6's composer state).
4. **`kata-improvement`** — accepted findings become target conditions
   and one experiment at a time on board tasks (the review is the Check;
   the improvement is the Act). Do not open experiments for findings the
   report merely proposes — the operator adjudicates at the algedonic
   review; the report proposes, the board carries what is accepted.
5. **`skill-discovery`** if a gap appears no installed skill covers —
   route it honestly rather than improvising a method.

## Verified ground truth (checked 2026-11-06, re-verify at pickup)

- **Repo:** `/home/mdz-axolotl/Clones/zed-kask` — a SHARED tree, multiple
  streams active. Pathspec commits only; check `git diff --cached --stat`
  before any commit; verify the commit numstat against the measured
  pre-commit diff; never a working-tree-wide mutation; one repository,
  never a second clone. Other streams' uncommitted files are untouchable.
- **Landed (do not redo):** S1 `828b9c09f7`+`f0a3cbd569`; S2
  `df72da1487`+`8773678abd`; S3 `b01e4a168b`+`a23af25a8d`; S4
  `513977507d`+`a654d12780` (widget 71/71 + 1 ignored on a fresh
  baseline run); S5 `76b6b99907` (panel 126/126 + clippy clean on
  fresh runs, red-first documented: 26 compile errors naming the missing
  surface before the green); S7's server half `83d122809c`
  (`educt_export` formats webvtt, web_annotation, otio — symbols in
  `transcript_export.rs`, `types.rs`, `templates.rs`, `tools/educt.rs`).
- **Board:** S1–S5 done, S6 backlog (unclaimed at compose), S7 in_progress
  (the S7 stream's panel half presumably remaining — their files
  `hkask_mcp_media.rs` + `tools/voice.rs` were dirty at compose time).
- **The goal `70f687af-2783-4e3c-b4a0-a8571c2448f1` is CLOSED** (operator,
  `accomplished_achieved`, 2026-11-06T06:06:17Z) on the closure working
  paper's evidence, which honestly recorded S5/S6/S7 undelivered at ITS
  snapshot — S5 and S7's server half landed after it. Consequences for
  this review: NO goal operations (the system refuses judging closed
  work); findings route to the BOARD and the operator. The native
  correction path (`kanban_goal_correct_outcome`) exists if the operator
  wants the closure record to account for post-snapshot deliveries — the
  operator's call alone, never the reviewer's.
- **CI at compose:** the newest run (`42174671752`, head `c10f39bb1d`) was
  still running; the two below it failed (`42170125609` on `a8426d25e7` —
  a docs-head run; `42168894399` on `3547a133ef`). Adjudicate whose
  failure before reacting: `gh run view <id> --repo mdz-axo/zed-kask
  --log-failed`; CI fails fast (while `check` fails, `kask-tests` is
  cancelled); the media layout suite has a known loaded-runner flake
  class — re-run in isolation before blaming a change.
- **Compose-path anchors, verified at `c10f39bb1d`**
  (`crates/media_panel/src/media_viewer.rs` — lines drift, re-verify):
  `reel_selection` :880, `compose_workspace_reel` :3949,
  `render_workspace_reel_section` :7836, `ReelComposeMode` :9379,
  `union_word_ranges` :9427, `compose_reel_params` :9880. The S5
  interaction machinery: `selection_tray_mode` :9091,
  `dispatch_workspace_correction` :3588,
  `apply_workspace_correction_result` :3774, `popover_correct` :4485,
  `render_selection_popover` :4958.
- **Gates at compose (fresh runs, no inherited green):**
  `cargo test -p media_panel` 126/126; `cargo test -p hkask-media-widget`
  71 passed + 1 ignored; the server suite carries S7's new fixture pins —
  run `cargo test -p hkask-mcp-media` and record the count.
- **End-to-end asset:** the Dunning recording — gallery asset
  `dunning-knowing-yourself-2017.mp4`
  (id `80f804a6-f5da-4119-bcfc-292403b3dd61`), transcript
  `5509bb94-6a9e-41b3-afb9-512e4db618e1` (14,270 words, `re_anchored`,
  ~82.7 min). S6's acceptance-4 round trip and the review's
  live-composition exercise use it.

## Deliverables and routing

1. **The review report**, committed as a doc (follow the tree's review
   convention — the working paper lives under `kask/docs/research/`;
   check for a reviews location at pickup and say where it landed).
   Contents: the machine-check receipts; findings with file:line,
   severity, IS/OUGHT adjudication, and a falsifier each; the
   composition-pattern analysis over the eight axes; the S6-design
   pre-review section (findings-on-paper, each tagged DESIGN vs CODE);
   the AI-code annex; the decoupling disclosure. An operator-facing
   reading surface must be verified to exist in the same turn it is
   offered — the report's path, line count, and a re-runnable check
   command ride in the handoff message itself.
2. **Findings routing:** actionable findings become board cards (the
   existing board `8820ae83-a57f-45d6-9caf-23320e687ca8`) with kata
   records once accepted — the report PROPOSES with severities; it does
   not mass-create work the operator has not adjudicated. Any fix the
   review itself lands (trivial, unambiguous, in its own write scope):
   red pin first, gates re-run, pathspec commit, net lines +
   replaces/deletes reported.
3. **No goal operations.** No auto-approval of S6/S7 landing claims —
   re-run the oracles against what you review; a claim inherited from an
   earlier green state is a fabricated attestation (`.rules`).
4. **`.rules` additions** proposed in the change report, never edited
   inline during review work.

## Binding process directives

- **Machine checks before judgment:** `cargo test -p media_panel`,
  `-p hkask-mcp-media`, `-p hkask-media-widget` (if touched);
  `./script/clippy -p <crate>` (the script carries the deny flags —
  do NOT pass `--deny warnings` on the command line; it splices them into
  the wrong position and fails). A plain `cargo check` compiles neither
  `#[cfg(test)]` code nor `tests/` — not a clean claim. Live-mutation
  probes run `--test-threads=1`.
- **The oracle re-runs against the change claiming it** — no inherited
  green; receipts retained in the report.
- **Shared tree:** staged-index check before any commit; pathspec commits
  (`git commit -m "..." -- <your files>`); numstat verification against
  the measured pre-commit diff; NEVER working-tree-wide mutations (no
  bare `git stash`/`restore`/`checkout`/`reset --hard`; `cargo fmt --all`
  is the same class — format only files you measured dirty). Other
  streams' uncommitted files are untouchable; their unpushed commits are
  never pushed or touched.
- **Kanban:** one state-mutating call per task per batch (moves AND
  comments race); kata records on tasks the review opens or works; Done
  is reachable only through `kanban_task_verify` with evidence, from
  `review` (not `in_progress`).
- **edit_file discipline:** `old_text` verbatim from a FRESH read;
  re-read the landed bytes after edits (the tool can land a different
  byte arrangement than `new_text`, and its fuzzy matcher applies
  near-miss `old_text` — two live near-misses in the S5 session were
  caught only by the compiler and a `git diff` audit); never chain
  byte-assumed anchors across edits on the same file; scripted per-item
  edits pin every definition form first and re-read the landed bytes
  after. A pre-edit read is approximate ground — verify near-miss
  regions against `git diff`, not reads alone.
- **GPUI traps:** timers via `cx.background_executor().timer` (NEVER
  `smol::Timer`); no `block_on` on the foreground; `AsyncApp` is not
  `Send`; `min_h_0`/`min_w_0` on shrinking containers; absolute children
  only under explicit `relative()` anchors; windowed layout tests need
  `init_layout_test_globals(cx)` (+ `admit_fixture()` where the fixture
  renders); the test harness runs effect-driven draws only.
- **Goal-intake session identity for any new records:** curator thread
  `8314713b-0963-4787-8b4a-ba64689562aa` (never invent a source locator).

## Known traps specific to this review

- The jscpd duplication ratchet: jscpd's JSON reporter emits path
  SUFFIXES — resolve report names by unique-suffix match, never
  repo-root paths. S6 is the plan's only L-slice: any accepted-duplication
  baseline bump lands in the commit that earns it.
- The media suites' loaded-runner flake class
  (`media_widget::layout_tests::video_area_scales_with_window_size`,
  `educt_render_edl_produces_a_clip_from_a_real_media_file` in full-suite
  runs) — re-run in isolation before blaming a change.
- CI fails fast: while the `check` job fails, `kask-tests` is cancelled —
  adjudicate whose failure before reacting.
- `InteractiveText` fires `on_click` only when mouse-down AND mouse-up
  land in the same range — cross-word press/release is click-free by
  construction (the drag pattern; banked in memory
  `media_panel:drag_selection_pattern`).
- Layout floors belong to the embedding site, never the shared per-asset
  widget entity (memory `media_panel:layout_floor_discipline`).
- The correction working view is newest-layer-wins: the S5 merge carries
  disjoint priors verbatim so the newest layer never silently reverts an
  earlier fix. If S6's strike math needs similar merge/supersession
  semantics, reuse `merge_over_prior` or justify the fork.
- A `~`-rooted path literal in Rust never expands; a zero-result over such
  a probe is a broken instrument, not absence.
- The repo-root `.rules` is the only `.rules` loaded; nested ones are never
  read.
- STT output is the model's claim about audio, not evidence a person spoke
  — irrelevant here, but it is the standing example of claim-vs-evidence
  discipline the report's receipts should hold.

## First actions at pickup

1. Re-verify the ground truth: `git --no-optional-locks status --short`;
   `git --no-pager log --oneline -8`; has S6 been claimed or landed? has
   S7's panel half landed? (`kanban_task_list` on board `8820ae83`;
   `git log --oneline -- crates/media_panel kask/mcp-servers/hkask-mcp-media`);
   the CI state (`gh run list --repo mdz-axo/zed-kask --workflow
   "Kask invariants" --branch main --limit 3`) — adjudicate any red
   before reacting, and re-verify every line anchor above (they drift).
2. Read the required list (`.rules`, the skills, the plan §4 S6/S7, the
   study §4/§6/§7, reference.md v3.4.0, the closure working paper).
3. Run the machine checks over the review scope (tests + clippy for
   `media_panel`, `hkask-mcp-media`, and `hkask-media-widget` if touched)
   — receipts into the report before any judgment is written.
4. Then the multi-axis review over the eight axes, the S6-design
   pre-review, the report, the findings routing — and the report's path,
   line count, and a re-runnable check command in the handoff message.
