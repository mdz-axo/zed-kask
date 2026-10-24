# Reduct.cloud removal plan — 2026-10-22

Operator-authorized removal of the live Reduct.video cloud capability from the
media MCP server, collapsing it to the local substrate. This artifact records
the census, the ranked plan, the plan-gate verdicts, and the execution record.
It predates the first destructive edit (mtime is the record).

## Goal and done means

The media capability becomes exactly the local substrate: research-evidence
ingestion, transcription with word-level timings, the educt layer system
(correction/speaker/paragraph/highlight/EDL), local reels, gallery annotation,
jobs, A2A/faces/images/assets. Zero LIVE Reduct references remain (client,
tools, credential chain, settings row, provider profile, panel group,
connection docs); reference-model citations REMAIN and are counted. Tool
enumeration delta = exactly the 20 `reduct_*` tools; every local flow passes;
`check-provider-profiles.sh` exits 0; dependents build clean.

## Keep/remove rule (operator instruction)

- REMOVE (live-connection class): API client, `reduct_*` tool handlers,
  credential (keychain slot `kask://credentials/reduct_api_key`, env
  `REDUCT_API_KEY`, allowlist entries, Data Services descriptor + row),
  provider profile, panel Reduct group, connection docs, live-tool
  instructions in skills/templates.
- KEEP (reference-model class): the transcript-linked-media reference model
  (operator ruling 2026-09-25; Berthouzoz et al. 2012 EDL anchor), citation
  prose, MovieLabs OMC provenance vocabulary, the published-model URLs, the
  bridge-ontology `derived.rs` concept. Zeroing these out is over-removal.

## Census (completed 2026-10-22; file:line anchors verified against the tree)

Baseline: generated `TOOL_NAMES` = 91 entries, exactly 20 `reduct_*`
(`target/release-mcp/build/hkask-mcp-media-*/out/tool_names.gen.rs`), pinned by
`tool_surface_pin!(..., 91, tool_surface_is_exactly_91_registered_tools)`
(`hkask_mcp_media.rs:456-461`) and by the live-router test
(`tests/schema_compliance.rs:57-63`). After: 71.

### REMOVE (live-connection class)

| Surface | Anchor |
|---|---|
| Client + 20 tool handlers + fixture tests (3,189 lines) | `kask/mcp-servers/hkask-mcp-media/src/tools/reduct.rs` |
| `pub mod reduct;` | `src/tools.rs:8` |
| `reduct_api_key` field (+doc) | `src/hkask_mcp_media.rs:175-177` |
| `+ Self::reduct_router()` | `src/hkask_mcp_media.rs:435` |
| `ctx.credentials.get("REDUCT_API_KEY")` init arg | `src/hkask_mcp_media.rs:621` |
| `CredentialRequirement::optional("REDUCT_API_KEY", ...)` | `src/hkask_mcp_media.rs:633-636` |
| OMC arms: CAPTURE :136-141, SEQUENCE :152-158, PARTICIPANT :165-167, STATE :168, highlights :169-171 | `src/omc.rs` |
| Dual-axis arms: REPORT :30-38, DATASET :90-100, prefix `"reduct_"` :135 | `src/ontology.rs` |
| 19 `Reduct*Request` structs + section comment | `src/types.rs:734-876` |
| `MAX_REDUCT_UPLOAD_BYTES` (+doc) — sole consumer is reduct.rs | `kask/crates/hkask-types/src/media_limits.rs` (deleted by this plan; then-lines 19–22) |
| Reduct schema/deser tests + imports + count pin 91 | `tests/schema_compliance.rs` |
| Cloud-connection README blocks (uncertain-POST/no-retry, tool contracts, upload/import, write surface) | `kask/mcp-servers/hkask-mcp-media/README.md:335-460`, `:84-86` |
| Reduct tool-reference section | `kask/docs/reference/mcp-servers/media.md:630-656` (+ :74, :78, :99, Transcript section :366-425 rework) |
| Data Services descriptor (label "Reduct.video", `shows_in_ui: true`) | `kask/crates/kask_bridge/src/inference_providers.rs:273-281` |
| Media allowlist entry + comment | `kask/crates/kask_bridge/src/mcp_servers.rs:358-359` |
| Reach test `reduct_key_reaches_only_media_child` → rewrite as ABW-style absence pin | `mcp_servers.rs:1117-1144` |
| Keychain purge (mirror ABW Decision A: `credentials.rs:44-89`) | new `purge_removed_reduct_api_key` + call in `crates/zed/src/main.rs:1182` + re-export `kask_bridge.rs:135` |
| Settings row: header doc `:1-2`, `storage_copy` special case `:98-104`, row test `:226-234` (→ absence pin) | `crates/settings_ui/src/pages/kask_page/data_services.rs` |
| Description + search alias "reduct" | `crates/settings_ui/src/pages/kask_page.rs:546,559` |
| Provider profile | `kask/registry/providers/reduct.yaml` (delete) |
| Script case arm `reduct_api_key) echo reduct` | `kask/scripts/check-provider-profiles.sh:66` |
| Panel: prefix lists `:223`, `:466`; group tuple `:253`; guidance sentence `:275-276`; count pin `:483` (→71); boundary-test entry `:543` (co-change with prompt sentence) | `crates/media_panel/src/media_panel.rs` |
| transcript-reel live-tool sentence `:17`; live-tool option bullet `:170-175` (rework, keep discipline) | `.agents/skills/transcript-reel/SKILL.md` |
| Live-tool instructions/inventory claims | onboarding `:31, :64-65, :143, :229, :259-262`; `mcp-servers/README.md:39` media row; root `README.md:35,45,47`; `diataxis/media_panel/reference.md:47`; `diataxis/kask_bridge/reference.md:259-261`; `.github/workflows/kask-invariants.yml:295` ffprobe rationale |
| Historical integration plan doc (instructs toward removed capability) | `tasks/reduct-cloud-integration-plan.md` (delete) |

### KEEP (reference-model class — counted after the sweep)

`bridge-ontology/src/derived.rs:703-716` (the `transcript_linked_media` ontology concept — the published anchor); `transcript-reel/SKILL.md:3` (description citation); media server `README.md:296-319` + media.md `:362-365, :444-446` (model + published URLs); `transcript_layers.rs:17,93`; `transcript_pass.rs:570`; onboarding `:23, :62`; DIVERGENCE model citations; the dated historical records (media.md Redesign record, loop-register :1286, docs-alignment-run.md, onboarding dated receipts) — history, not instructions.

### Excluded (false positives / incidental — do not chase)

`reduction_pct`/reduction English words (condenser engine/types/README, condenser_bridge, kask_compaction, thread.rs, salience.rs, reversal.rs, bundle_commits.rs, scenarios server, .wezel/schema.json, testing-protocol.md, compaction-pipeline-spec.md, condenser reference.md, vector-store doc, essentialist/verification-compression/metacognition/adhd-mode/deep-module/improv/kanban/therapy/local-research-swarm skills + their templates — all "reduction" vocabulary, zero reduct-tool references); SUMO/CSO ontology sources (dimensionality reduction); `"Play in Reduct"` publication-chrome marker (`hkask-memory/src/text_chunking.rs:533,1383-1394` + `kask/scripts/audit/calibrate-chunk-retrieval.sh:512` — live functional fixture, removing it would break chrome-stripping and the audit parity check). `.rules`: zero reduct hits (the issuing prompt's "at least one" does not reproduce — verified `grep -cin reduct .rules` → 0).

### Consumer census (name-visibility, incl. defining crate src/ + tests/)

`reduct_router`: reduct.rs (deleted by this plan; definition at then-line 1217), hkask_mcp_media.rs:435 (sole caller). `Reduct*Request`: reduct.rs + schema_compliance.rs only. `MAX_REDUCT_UPLOAD_BYTES`: reduct.rs (then-line 1562) + media.md (then-line 793 — a dated Redesign record since removed). `reduct_api_key` field: reduct.rs + lib-root init. `REDUCT_API_KEY`: mcp_servers.rs, inference_providers.rs, data_services.rs, reduct.rs, docs. reqwest stays (assets.rs, gallery/vision.rs, tools/youtube.rs); sha2 stays (six non-reduct consumers). No orphan Cargo deps.

## Phases (dependency-ordered; per-phase verification gate)

- **A — Server core:** delete `tools/reduct.rs`; prune `tools.rs`, lib root (field, router, credential init, CredentialRequirement, pin 91→71), `omc.rs` arms, `ontology.rs` arms + prefix, `types.rs` structs, `media_limits.rs` const, `schema_compliance.rs` (imports, 91→71 + retired-list gains the 20 names, schema/deser tests, comments), media README blocks. Gate: `cargo build -p hkask-mcp-media` + `cargo test -p hkask-mcp-media` green; regenerated `tool_names.gen.rs` diff = exactly the 20 reduct names.
- **B — Panel light-touch:** remove the group tuple, both prefix-list entries, the guidance sentence, count pin → 71, boundary-test entry (co-change with the prompt sentence). No structural collapse; every local surface stays. Gate: `cargo test -p media_panel` green.
- **C — Settings/credentials:** kask_bridge descriptor + allowlist + test rewrite; `purge_removed_reduct_api_key` + main.rs call + re-export; settings_ui header/storage_copy/test; kask_page description + alias. Gate: `cargo check -p kask_bridge -p settings_ui` + targeted tests green.
- **D — Provider profile:** delete `reduct.yaml`, script arm; Gate: `bash kask/scripts/check-provider-profiles.sh` exit 0.
- **E — Skills/docs/DIVERGENCE:** transcript-reel rework; delete `tasks/reduct-cloud-integration-plan.md`; media.md/onboarding/README reworks (counts 91→71, media rows, fleet total −20 with staleness flagged); workflow comment; DIVERGENCE D5 (enumeration + purge record), D9 block rewrite, D35 phrases, D35/D9/D2 block. Gate: two-class sweep.
- **F — Final:** full-repo symbol sweep; `./script/clippy --deny warnings` touched scope; `cargo test -p hkask-mcp-media -p media_panel`; `cargo check -p kask_bridge -p settings_ui`; `cargo check -p zed`; workspace build; before/after enumeration; net lines.

## Load-bearing hypotheses (hypothesis-framer) and discriminating tests

- H1 `tools/reduct.rs` is the sole definition site of all 20 tools + router. Test: post-deletion build + `combined_router()` pin green; repo-wide `reduct_` sweep classifies every survivor.
- H2 No non-reduct consumer reads `reduct_api_key`/`REDUCT_API_KEY`. Test: census (done) + green build after field removal.
- H3 The panel's reduct group has no local consumers. Test: panel tests green with tuple + prefixes removed; `render_grouped_tool_advertisement` renders from TOOL_NAMES by construction.
- H4 `check-provider-profiles.sh` stays coherent without reduct.yaml. Test: exit 0, adopted count drops by one.
- H5 TOOL_NAMES delta = exactly the 20 reduct names, 71 total. Test: regenerated gen-file diff + both pins.
- H6 Stored educt layers / OMC records parse unchanged (no typed reduct structs in stored records; provenance fields are plain strings). Test: local-flow test suite green.
- H7 (null) Removing reference-model citations would NOT advance the local-only target — it would violate the keep rule. Test: final sweep counts the surviving citation set.
- H8 Fleet-total arithmetic is derivable, not estimated. Test: media pin 71; apply −20 to the "343" fleet claims and reconcile per-row counts; pre-existing per-row staleness (102 vs 91) flagged to the operator, not silently rewritten.

## Metacognition verdict — confidence per phase

A server core 0.93 (self-contained module, enumerated consumers); B panel 0.90; C credential chain 0.85 (three crates + test rewrites); D profile 0.95 (the script's own zombie scenario); E skills/docs prose 0.75 (two-class boundary is judgment — mitigated by the enumerated survivor list + counted sweep); F verification 0.90 (time risk, not correctness). Calibration check: pre-declared expectation = enumeration delta exactly 20 and pins at 71; measured by the regenerated gen file, not by assertion.

## Grill-me verdict

Recall — the full REMOVE/KEEP/EXCLUDED inventory above, from the live tree, not the issuing prompt (two prompt claims corrected: `.rules` has zero reduct hits; the "example-illustration" skills are English-word false positives with zero live-tool references). Mechanism — build.rs scans `#[tool]` fns into TOOL_NAMES; the pin macro checks count + set against the live router; the panel renders its advertisement from TOOL_NAMES; credentials flow keychain → allowlist filter → child env → `ServerContext`; DATA_SERVICES drives the settings rows and the profile script's derived adopted set. Rationale — reference-model citations anchor the local design on a published ontology concept (`derived:transcript_linked_media`); live-connection code has no remaining purpose under the local-only goal. Edge cases — unprefixed credential key; reqwest/sha2 retention; fixture strings; dated historical records kept; DNS-rebinding boundary is yt-dlp/FFmpeg-scoped (onboarding item 5) and SURVIVES — only the Reduct-specific "cloud no-retry" clause goes; shared tree clean at start (ABW landed through `7df4743c38`, trust-verdict stream committed) — re-diff before every shared-file edit; no commits unless asked. Verdict: PASS.

## Essentialist verdict — Exist/Surface/Contract per item

`tools/reduct.rs` Exist: deletion loses real behavior — the removal IS the operator's order; Surface: −20 tools, −1 router, −1 field, −19 request structs, −1 const; Contract: no local flow imports it. Credential chain Exist: purge mirrors ABW Decision A (dormant third-party secret with no read path is worse than a logged one-time delete); no kept-for-later toggle, no compatibility stub. Panel Exist: removing the tuple removes zero local behavior (group rendering is generic); no reduct-only shared helper exists. Docs Exist: connection sections go; dated records stay (deleting history loses receipts — not this task). Deletion-test load-bearing couplings the plan must state (Guardrail, REQUIRED): (1) panel prompt sentence ↔ panel boundary-test entry `:543` — removing one without the other breaks the build; (2) `storage_copy` conditional removal must take the whole branch, not leave a dead `if`; (3) every reach-pin replaced by an absence pin in the same phase (ABW precedent `mcp_servers.rs:1221-1223`); (4) every file:line cite my line-shifting edits invalidate gets refreshed in the same pass (media.md `:56, :99` cites). Anti-over-removal contract: the KEEP set above is enumerated; zeroing it is a Prohibition violation. Verdict: PASS — zero Prohibition findings; the four Guardrail co-changes are folded into the phases.

## Bounded capabilities review (one list, no further iteration)

- code-review — on the final diff: removal diffs are where orphan piles and compatibility states hide; machine checks over the actual change. ADDED (runs at Phase F).
- grounding-verify — on the final report: every count and file:line claim traced to the tree; the report's delta/citation/net-line numbers are the deliverable. ADDED (runs at Phase F).
- kask-seam-audit — SKIPPED: the panel edit is a group-label/prefix removal inside the existing D-seamed surface; the audit's measured-layout loop and multi-track sweep exceed a removal whose panel scope is explicitly light-touch.
- deep-module — SKIPPED: the deletion test is applied inline via essentialist on every plan item (the gate folds it in); no new module design is being made.

## Verdicts

metacognition: PASS (confidences above). hypothesis-framer: PASS (H1–H8, discriminating tests bound to phases). grill-me: PASS. essentialist: PASS (0 Prohibition, 4 Guardrail co-changes folded in). Capabilities: code-review + grounding-verify added for Phase F. **Final: ACCEPT — proceed to execution (operator pre-authorized in the issuing session).**

## Projected net lines

−3,189 (reduct.rs) −143 (types.rs) −4 (media_limits) −~110 (schema tests) −~36 (omc) −~26 (ontology) −~8 (lib root) −~130 (README cloud docs) −~95 (media.md) −6 (panel) −11 (kask_bridge removals) +40 (purge fn) −~13 (settings_ui) −~10 (skills) −~75 (tasks plan delete) −45 (reduct.yaml) ±0 (DIVERGENCE rewrites) −~20 (onboarding) ≈ **net −4,000 ± 300** (the plan artifact itself is +~200 and is not part of the removal count; reported separately).

## Execution record

- **Gate order held:** this artifact (with all five verdicts) was written before the first destructive edit; Phase A began after it.
- **A — server core:** reduct.rs deleted (−3,189); tools.rs, lib root, omc.rs, ontology.rs, types.rs (−144), media_limits, schema_compliance reworked. 13 test-constructor call sites updated for the 9-arg `MediaServer::new` (rg's first sweep missed 4 sites in `tools/` — found via the compiler). Gate: `cargo test -p hkask-mcp-media` serial **311/311 green**; regenerated `tool_names.gen.rs` = **71 entries, 0 reduct, delta exactly 20 removed / 0 added** (diff vs the pre-change release-mcp baseline). One parallel-mode flake observed twice (`educt_render_edl_produces_a_clip_from_a_real_media_file` — a shared-temp-dir cleanliness assertion racing sibling tests; passes solo and serial; my change touches no ffmpeg/temp logic — flagged, not a regression).
- **B — panel:** group tuple, both prefix lists, guidance sentence, count pin → 71, boundary-test entry removed (the co-changed pair). Gate: `cargo test -p media_panel` **36/36**.
- **C — settings/credentials:** descriptor + allowlist entry removed; `reduct_key_reaches_only_media_child` rewritten as the absence pin `reduct_key_reaches_no_child`; `purge_removed_reduct_api_key` added (ABW mirror) + `crates/zed/src/main.rs` call + re-export; settings_ui header/storage_copy/row-test (→ `removed_reduct_cloud_row_is_absent`); kask_page description + alias. Gate: `cargo check -p kask_bridge -p settings_ui` clean; targeted kask_bridge tests **41 green**; settings_ui **77/77**.
- **D — provider profile:** `reduct.yaml` deleted, script arm removed. Gate: `bash kask/scripts/check-provider-profiles.sh` **exit 0, 16 adopted vendors** (reduct dropped from the derived set).
- **E — skills/docs/DIVERGENCE:** transcript-reel reworked (model paragraph + constraints bullet; description untouched); `tasks/reduct-cloud-integration-plan.md` deleted; media.md reworked (blocks deleted by pattern-anchored sed with count pre-checks; cites refreshed: struct :153-176, router :422-437, pins :439-515, run :519-660, omc :100-162, mcp_servers :354-358); onboarding reworked (section delete + the no-"reduct"-token "Read paths plus scoped write contracts" paragraph — found by the cloud-prose adjudication sweep, not the string sweep; count 71; receipts reworded; priorities renumbered; historical block given a dated removal note); registries/READMEs updated (media row 71, fleet −20); workflow ffprobe rationale reworded; DIVERGENCE D5/D9/D35 reconciled.
- **F — final:** full-repo sweep classified every survivor (removal records, reference-model citations, dated history, incidental fixtures, published-ontology sources — zero live-connection references); clippy `--deny warnings --all-targets --all-features` over the five touched kask crates **passed** (cargo-machete clean — no orphan deps); `cargo check -p zed` **green**; `cargo check --workspace` **green**; serial media suite + panel re-run green after the final code-file edit.
- **Shared-tree event (reported, not mine):** commit `b137123374` ("Remove Reduct.cloud integration from media server", +283/−3,972 incl. this plan doc) was created and pushed to origin/main at 11:26 by the operator side mid-Phase-E. I ran no git mutations. The four files edited after that snapshot (DIVERGENCE.md seam reconciliation, media.md cite refresh, onboarding fixes, educt.rs comment reword) remain uncommitted on top — the pushed commit's DIVERGENCE.md still records the pre-removal seams (11 reduct mentions), so the tail should land for record consistency.
- **Net lines:** commit −3,689 net + tail −5 net = **−3,694 total**, or **−3,824 excluding this plan artifact (+130)** — inside the projected −4,000 ± 300.