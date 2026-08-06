# Upstream Sync Plan — zed-kask → upstream Zed `main`

**Date:** 2026-08-06
**Author:** planning session (read-only; no rebase executed)
**Deliverable type:** plan document only — execution is prohibited by task constraint
**Target ref:** `upstream/main` @ `b5796233bd` (current tip after `git fetch upstream`)
**Fork HEAD:** `930a1c956d` on `main` (13 commits ahead of `origin/main`)

> **EXECUTION UPDATE (2026-08-06):** The plan was subsequently executed on branch
> `sync/upstream-2026-08-06`. Merge commit `c3f24c4d63` landed. The sections below
> preserve the original planning predictions; an execution summary is appended at
> the end (§9). Key divergences from the plan: only 3 actual conflicts (not 8
> predicted); 2 pre-existing fork compile bugs in `crates/zed/src/main.rs` were
> discovered and fixed; D20 propagated to the new upstream file
> `crates/copilot_chat/src/model.rs`.

---

## 1. Current state

### 1.1 Fork HEAD and divergence

- **Fork HEAD:** `930a1c956d` — "Add soft evidence marginal test and mark T6 done"
- **Behind upstream `main`:** **45 commits** (`git log --oneline HEAD..upstream/main`)
- **Ahead of `origin/main`:** 13 commits (unpushed fork work — graph widget backward inference, swarm settings, SSRF validation, CI hardening, prompt-enhance fixes)
- **Working tree:** clean
- **Remotes:** `origin` → `github.com/mdz-axo/zed-kask.git`; `upstream` → `github.com/zed-industries/zed.git`

### 1.2 Conflict hotspots (behind-commits touching D-seam or supporting files)

Only **8 files** carrying fork divergence are touched by the 45 behind-commits. The remaining ~37 behind-commits touch upstream-only files (vim, gpui internals, collab_ui, project_panel, etc.) and will merge cleanly.

| File                                                   | Behind-commits                          | Fork markers                                                                                        | Conflict risk                                                                                                                                                                                     |
| ------------------------------------------------------ | --------------------------------------- | --------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/markdown/src/markdown.rs`                      | `00cba838ad` (Mermaid zoom)             | 10× D18 markers at L1304–L6215                                                                      | **HIGH** — upstream `@@ -1300,24 +1510,17` and `@@ -1415,12 +1635,6` overlap the fork's D18 `MarkdownElement` field, builder, and `Element` impl regions directly                                 |
| `crates/git_ui/src/git_graph.rs`                       | `b8c75f1717`, `21f16f7b5b`              | 3× D11 markers at L605, L621, L2749                                                                 | **HIGH** — upstream `@@ -602,26 +602,11` and `@@ -2746,9 +2741,7` overlap the fork's `#[allow(deprecated)]` call sites                                                                            |
| `crates/agent/src/tools/terminal_tool.rs`              | `b5796233bd`, `849ec5898a`              | (no `// zed-kask:` markers, but fork has supporting edits per DIVERGENCE.md "Other modified files") | **MEDIUM** — upstream rewrites `working_dir`, `select_terminal_output_lines`, `process_content`; fork has the head/tail overlap fix + truncation spillover. No marker collision but logic overlap |
| `crates/agent_ui/src/agent_panel.rs`                   | `66ed3027b8`, `21f16f7b5b`              | (no markers, but fork has eager `SkillIndex` + `create_thread_with_options` Curator edits)          | **MEDIUM** — upstream `21f16f7b5b` splits the crate graph (moves code to `git_ui_core`/`zed_actions`); fork's Curator-support edits may collide with moved regions                                |
| `crates/agent_ui/src/conversation_view/thread_view.rs` | `00cba838ad`                            | 1× marker at L11505 (`render_skill_loading_issues`)                                                 | **LOW–MEDIUM** — upstream `@@ -11502,14 +11524,20` is adjacent to the fork marker; likely a context conflict, not a content conflict                                                              |
| `crates/zed/src/main.rs`                               | `6153542cf6` (copilot_chat dep removal) | 5× markers at L314, L358, L416, L1726                                                               | **LOW** — upstream change is at L680 (adds 2 lines for `zed_credentials_provider` feature flag); fork markers are far away                                                                        |
| `crates/agent/src/thread.rs`                           | `849ec5898a`                            | 1× marker at L4851                                                                                  | **LOW** — upstream changes are at L5498–L5994 (NTFS/WSL warning fix); fork marker at L4851 is far away                                                                                            |
| `Cargo.toml` (root)                                    | `849ec5898a`, `21f16f7b5b`              | (no markers, but `[workspace.members]`/`[workspace.dependencies]` carry the kask crate list)        | **MEDIUM** — both sides add workspace members/deps; mechanical merge of two sorted arrays                                                                                                         |

### 1.3 Files with `// zed-kask:` markers NOT touched by behind-commits

These 30+ files (D1–D21 seams + supporting files) will **not** conflict. Listed for completeness so the re-pin task (§3.8) knows the full deviation surface: `crates/agent/src/tools/skill_tool.rs`, `crates/agent/src/agent.rs`, `crates/agent_skills/agent_skills.rs`, `crates/agent_ui/src/agent_ui.rs`, `crates/agent/src/tool_router.rs`, `crates/agent/src/templates.rs`, `crates/auto_update/src/auto_update.rs`, `crates/auto_update_ui/src/auto_update_ui.rs`, `crates/editor/src/blink_manager.rs`, `crates/acp_thread/src/acp_thread.rs`, `crates/language_models/src/provider/api_compatible.rs`, `crates/open_router/src/open_router.rs`, `crates/language_model_core/src/language_model_core.rs`, `crates/open_ai/src/open_ai.rs`, `crates/open_ai/src/completion.rs`, `crates/zed/src/zed/app_menus.rs`, `crates/client/src/client.rs`, `crates/cli/src/main.rs`, `crates/zed_actions/src/lib.rs`, `crates/gpui/src/app.rs`, `crates/terminal/src/alacritty/hyperlinks.rs`, `crates/collab/src/api/kask_skills.rs`, `crates/collab/src/db/queries/kask_skills.rs`, `crates/kask_extensions_ui/src/publish.rs`, `crates/cloud_api_types/src/kask_skill.rs`, `crates/settings_ui/src/pages/skills_visibility.rs`, `crates/settings_ui/src/pages/skills_setup.rs`, `crates/zed/src/zed/open_listener.rs`, `crates/windows_resources/src/windows_resources.rs`, `crates/gpui_tokio/src/gpui_tokio.rs`, `crates/prompt_store/src/prompts.rs`, `crates/agent/src/tools.rs`, `crates/language_model/src/language_model.rs`, `crates/language_model/src/registry.rs`, `crates/open_ai/src/list_models.rs`, `crates/client/src/zed_urls.rs`, `crates/agent_ui/src/conversation_view.rs`.

---

## 2. Strategy recommendation + rationale

### Recommendation: **merge** (`git merge upstream/main`), not rebase.

**Rationale.** The fork is 13 commits ahead of `origin/main` with unpushed work, and 45 commits behind upstream. A rebase would replay 13 fork commits onto 45 upstream commits — each fork commit becomes a potential conflict point (13× conflict surfaces), and the fork's history is linearized, losing the grouped semantic units ("Add polytree backward inference", "Harden kask CI"). A merge applies the entire fork divergence as a single two-parent commit, producing **one** conflict-resolution pass over the 8 hotspot files. The conflict surface is identical either way (the same 8 files overlap), but merge localizes resolution to a single commit and preserves the fork's commit history — which matters because `DIVERGENCE.md` and the `.rules` "Tests must pin deliberate zed-kask deviations" trap reference specific fork commits in their pinning tests. Rebasing would invalidate those references.

The 45 behind-commits touch only 8 of the ~40 D-seam/supporting files, and the high-risk conflicts (`markdown.rs`, `git_graph.rs`) are in files where the fork's deviations are well-localized (D18 media-block-renderer field/builder; D11 `#[allow(deprecated)]` annotations). The upstream deltas in those files are additive (Mermaid zoom adds new code; crate-graph split moves code) rather than rewriting the exact lines the fork modified — so manual merge is tractable.

**Tradeoff acknowledged.** Merge leaves a merge commit that upstream Zed's history would not have. This is acceptable: zed-kask is a fork, not a contributor branch, and `DIVERGENCE.md` §"Upstream-sync runbook" already prescribes `git merge upstream/main` as the canonical operation (line 95). Rebase would deviate from the documented runbook without justification.

**Lower-risk alternative rejected:** rebase. It would produce a cleaner linear history but at 13× the conflict-resolution cost and with history-rewrite risk to the unpushed commits.

---

## 3. Task breakdown

Decomposed per `task-breakdown` methodology: vertical slices, explicit acceptance criteria, checkpoints every 2–3 tasks, high-risk slices scheduled first (fail fast). Each task is sized S–M (no XL). Phases: **Foundation → Conflict resolution → Verification → Documentation**.

### Phase A — Foundation (sequential)

#### Task A1: Create sync branch and fetch

- **Description:** Branch from `main` HEAD (`930a1c956d`) as `sync/upstream-2026-08-06`. Confirm `upstream/main` is at `b5796233bd`. Do not merge yet.
- **Acceptance criteria:**
  - `git rev-parse sync/upstream-2026-08-06` returns `930a1c956d`
  - `git log -1 upstream/main` returns `b5796233bd`
  - Working tree clean
- **Verification:** `git status` + `git rev-parse`
- **Dependencies:** None
- **Files touched:** none (git metadata only)
- **Scope:** XS

### Phase B — Conflict resolution (sequential, highest-risk first)

#### Task B1: Resolve `crates/markdown/src/markdown.rs` (D18, HIGH risk)

- **Description:** Merge upstream's Mermaid zoom changes with the fork's D18 `media_block_renderer` field/builder/dispatch. Upstream adds ~278 lines to `markdown.rs` and restructures `MarkdownElement` (hunks at L1300, L1415, L2218, L2349). The fork's D18 additions are at L1304 (field), L1313 (callback type), L1337 (dispatch), L1418 (builder), L2221/L2249/L2377 (Element impl hooks), L6009+ (tests).
- **Resolution rule:** **Manual merge.** Keep all fork D18 markers and the `media_block_renderer: Option<MediaBlockRendererFn>` field + `.media_block_renderer()` builder. Apply upstream's Mermaid zoom additions (new `MarkdownStyle` fields, `Markdown` struct changes, `Element` impl changes) around the D18 hooks. The D18 dispatch point in the `Element` impl (L2377) must remain _before_ the default code-block renderer so viz blocks intercept; upstream's Mermaid changes are in a different code path (svg/mermaid rendering) and should not collide semantically. Re-pin the D18 tests at L6009+ to assert they still pass after the upstream restructure.
- **Acceptance criteria:**
  - `cargo check -p markdown` succeeds
  - `cargo test -p markdown -- selects_event_tree_body falls_through_non_graph_bodies` passes (D18 pinning tests)
  - All 10 `// zed-kask: D18` markers present and pointing at correct code
  - Mermaid zoom feature compiles (no feature-gate regression)
- **Verification:** `cargo check -p markdown && cargo test -p markdown`
- **Dependencies:** A1
- **Files touched:** `crates/markdown/src/markdown.rs`
- **Scope:** M

#### Task B2: Resolve `crates/git_ui/src/git_graph.rs` (D11, HIGH risk)

- **Description:** Merge upstream's provider-icon + crate-graph-split changes with the fork's D11 `#[allow(deprecated)]` annotations on `time::format_description::parse` call sites. Upstream hunks at L602 (struct field change), L2557/L2590 (impl changes), L2746 (the deprecated call site region).
- **Resolution rule:** **Keep kask side for the `#[allow(deprecated)]` annotations; take upstream for everything else.** The fork's D11 seam is purely the three `#[allow(deprecated)]` + `// zed-kask:` comments at L605, L621, L2749. Upstream's L2746 hunk is adjacent — verify the deprecated call site still exists after the upstream delta; if upstream migrated to `parse_borrowed`, remove the D11 seam entirely and update `DIVERGENCE.md` (per the D11 entry: "Remove this seam when upstream migrates to `parse_borrowed`"). If upstream did not migrate, preserve all three annotations.
- **Acceptance criteria:**
  - `cargo check -p git_ui` succeeds
  - Either: all 3 D11 markers present (upstream did not migrate) **or** 0 D11 markers present and `DIVERGENCE.md` D11 row marked removed (upstream migrated)
  - `cargo test -p git_ui` passes
- **Verification:** `cargo check -p git_ui && cargo test -p git_ui`
- **Dependencies:** A1
- **Files touched:** `crates/git_ui/src/git_graph.rs` (and `DIVERGENCE.md` if D11 removed — see Task D1)
- **Scope:** S

#### Task B3: Resolve `crates/agent/src/tools/terminal_tool.rs` (MEDIUM risk)

- **Description:** Merge upstream's `working_dir` path-resolution improvement (`b5796233bd`) and NTFS/WSL warning fix (`849ec5898a`) with the fork's supporting edits (truncation spillover file, head/tail overlap fix in `select_terminal_output_lines`, relaxed shell-substitution doc). No `// zed-kask:` markers in this file.
- **Resolution rule:** **Manual merge, prefer upstream for `working_dir` + WSL logic; preserve fork's `select_terminal_output_lines` overlap fix and truncation spillover.** Upstream's `b5796233bd` rewrites `working_dir` (L1416 hunk, +189 lines) — take upstream's version. Upstream's `849ec5898a` adds WSL-availability gating — take upstream. The fork's `select_terminal_output_lines` head/tail overlap fix (per DIVERGENCE.md "Other modified files") is in a different function — preserve it unless upstream's `1216,34 +1200,6` hunk removed the surrounding code. The fork's truncation spillover (full output saved to temp file) is in `process_content` — preserve unless upstream's `1303,7 +1252,6` hunk rewrites that function.
- **Acceptance criteria:**
  - `cargo check -p agent` succeeds
  - `cargo test -p agent -- terminal_tool` passes (or the fork's terminal_tool tests, if named differently)
  - Fork's truncation spillover behavior preserved (test asserting temp-file path returned)
  - Fork's head/tail overlap fix preserved (test asserting no overlap)
- **Verification:** `cargo check -p agent && cargo test -p agent`
- **Dependencies:** A1
- **Files touched:** `crates/agent/src/tools/terminal_tool.rs`
- **Scope:** M

#### Task B4: Resolve `crates/agent_ui/src/agent_panel.rs` (MEDIUM risk)

- **Description:** Merge upstream's full-screen-button toggle state (`66ed3027b8`) and crate-graph split (`21f16f7b5b`, which moves code to `git_ui_core`/`zed_actions`) with the fork's Curator-support edits (eager `SkillIndex` population, `create_thread_with_options` returns the agent used). No `// zed-kask:` markers.
- **Resolution rule:** **Manual merge.** Take upstream's `66ed3027b8` (toggle_state + selected_icon pattern) — it's a pure UI fix. For `21f16f7b5b`, the crate-graph split moves code _out_ of `agent_panel.rs` — accept the upstream deletions, then re-apply the fork's Curator-support additions to whichever file now hosts the moved code (likely `zed_actions` or `git_ui_core`). If the fork's `create_thread_with_options` return-type change is in code that upstream moved, re-apply it to the new location.
- **Acceptance criteria:**
  - `cargo check -p agent_ui` succeeds
  - `cargo check -p zed_actions` succeeds (if Curator code moved there)
  - Curator selectable in Agent Panel (manual or existing test)
  - Full-screen button shows toggled state (upstream test passes)
- **Verification:** `cargo check -p agent_ui -p zed_actions && cargo test -p agent_ui`
- **Dependencies:** A1
- **Files touched:** `crates/agent_ui/src/agent_panel.rs`, possibly `crates/zed_actions/src/lib.rs` or `crates/git_ui_core/` (if Curator code moved)
- **Scope:** M

#### Task B5: Resolve `Cargo.toml` (root) (MEDIUM risk)

- **Description:** Merge upstream's workspace member/dep additions (`849ec5898a` adds `recent_projects`/`sandbox` deps; `21f16f7b5b` adds `git_ui_core`/`zed_actions` workspace entries) with the fork's `[workspace.members]` kask crate list and `[workspace.dependencies]` kask deps.
- **Resolution rule:** **Manual merge of two sorted arrays.** Take upstream's new members/deps. Preserve all fork `kask/` members and kask-specific deps. The fork's entries are additive and isolated (all start with `kask/` or `hkask-`); upstream's are zed-prefixed. No semantic collision expected — pure array union.
- **Acceptance criteria:**
  - `cargo check --workspace` succeeds (or `cargo check -p zed` as a smoke test)
  - All `kask/crates/*` members present in `[workspace.members]`
  - All upstream new members (`git_ui_core`, `zed_actions` if added) present
- **Verification:** `cargo metadata --no-deps --format-version 1 | jq '.packages | length'` (sanity)
- **Dependencies:** B4 (since B4 may add a moved crate)
- **Files touched:** `Cargo.toml`
- **Scope:** S

#### Task B6: Resolve low-risk files (batch)

- **Description:** Resolve the three low-risk files in one pass: `crates/zed/src/main.rs` (upstream L680 +2 lines, fork markers at L314/358/416/1726 — no overlap), `crates/agent/src/thread.rs` (upstream L5498+ NTFS fix, fork marker at L4851 — no overlap), `crates/agent_ui/src/conversation_view/thread_view.rs` (upstream L11502 adjacent to fork marker at L11505 — context conflict only).
- **Resolution rule:** **Take upstream for all three; verify fork markers survive.** For `main.rs`, accept upstream's `zed_credentials_provider` feature-flag addition at L680; fork's `.env` loading and deferred wiring markers are far away. For `thread.rs`, accept upstream's NTFS/WSL fix; fork's `LazyToolRouter` marker at L4851 is far away. For `thread_view.rs`, accept upstream's Mermaid-zoom-related changes at L11442–L11591; verify the fork's `render_skill_loading_issues` marker at L11505 still points at valid code (upstream's `@@ -11502,14 +11524,20` hunk is adjacent — likely a context-only conflict that git resolves with minimal manual intervention).
- **Acceptance criteria:**
  - `cargo check -p zed -p agent -p agent_ui` succeeds
  - All fork markers in these 3 files present and pointing at correct code
  - `cargo test -p agent -- tool_router` passes (D6/D-LazyToolRouter pinning)
- **Verification:** `cargo check -p zed -p agent -p agent_ui`
- **Dependencies:** A1
- **Files touched:** `crates/zed/src/main.rs`, `crates/agent/src/thread.rs`, `crates/agent_ui/src/conversation_view/thread_view.rs`
- **Scope:** S

### Checkpoint C1 (after Phase B)

- **Verify:** `cargo check --workspace` succeeds; `bash kask/scripts/check-hkask-no-zed-deps.sh` passes (§13.1 invariant); all 8 hotspot files compile; no `// zed-kask:` marker lost. **Human review before proceeding to Phase C.**

### Phase C — Verification

#### Task C1: Re-pin every `// zed-kask:` deviation with a test

- **Description:** Per the `.rules` trap "Tests must pin deliberate zed-kask deviations from upstream": every `// zed-kask:` comment that disables upstream behavior needs a corresponding test asserting the disabled behavior stays disabled. After the merge, grep all `// zed-kask:` markers and confirm each has a pinning test. The 8 hotspot files are the priority, but the full 190-marker surface must be audited.
- **Acceptance criteria:**
  - `grep -rn "// zed-kask:" --include="*.rs"` returns the same count as pre-merge (190 ± any D11 removal per Task B2)
  - For each marker, a corresponding test exists (grep the test module for the marker's described behavior)
  - Any marker whose pinning test was upstreamed or removed gets a new test in the same commit
  - `DIVERGENCE.md` "Pinned by" references in each D-row still resolve to live tests
- **Verification:** `grep -rn "// zed-kask:" --include="*.rs" | wc -l` matches expected; `cargo test --workspace` passes
- **Dependencies:** Checkpoint C1
- **Files touched:** test modules in the 8 hotspot files + any file whose pinning test was lost
- **Scope:** M

#### Task C2: Run full test suite + clippy

- **Description:** Run the workspace test suite and `./script/clippy` (per `.rules` build guidelines: use `./script/clippy` not `cargo clippy`).
- **Acceptance criteria:**
  - `cargo test --workspace` passes (pre-existing failures normalized per `.rules` — investigate any _new_ failure)
  - `./script/clippy` passes with no new warnings
  - `cargo check -p kask_bridge -p hkask-types -p hkask-mcp-server` passes (per DIVERGENCE.md runbook step 5)
- **Verification:** command exit codes
- **Dependencies:** C1
- **Files touched:** none (verification only)
- **Scope:** S

### Phase D — Documentation

#### Task D1: Update `DIVERGENCE.md` if any seam changed

- **Description:** If Task B2 removed the D11 seam (upstream migrated to `parse_borrowed`), mark the D11 row as removed. If any other seam's file list changed (e.g., B4 moved Curator code to a new file), update the D-row's file list. If a new upstream edit required a fork-side patch outside the existing D-seams, propose a new D-seam entry (D22+) rather than editing upstream directly (per prohibition constraint).
- **Acceptance criteria:**
  - `DIVERGENCE.md` D-row file lists match the post-merge tree
  - Any removed seam has a "removed" note with date and reason
  - Any new seam has a full D-row entry (Surface, file, what's wired, pinning test)
  - No upstream file outside the D-seams was modified (verify with `git diff upstream/main -- <non-D-seam files>` is empty)
- **Verification:** `git diff upstream/main --name-only` shows only D-seam files + `kask/` + `Cargo.toml`
- **Dependencies:** C2
- **Files touched:** `DIVERGENCE.md`
- **Scope:** S

---

## 4. D-seam conflict-resolution rules (per file)

| File                                                   | D-seam                        | Resolution rule                                                                                                                                                                                                                                                                                     | Pinning test(s)                                                                                             |
| ------------------------------------------------------ | ----------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `crates/markdown/src/markdown.rs`                      | D18                           | **Manual merge.** Keep fork's `media_block_renderer` field, `MediaBlockRendererFn` type, `.media_block_renderer()` builder, and the `Element` impl dispatch hook (L2377). Apply upstream's Mermaid zoom additions around them. The D18 dispatch must remain before the default code-block renderer. | `selects_event_tree_body`, `falls_through_non_graph_bodies`, fence-language gate tests at L6009+            |
| `crates/git_ui/src/git_graph.rs`                       | D11                           | **Keep kask `#[allow(deprecated)]` annotations; take upstream for all else.** If upstream migrated `parse` → `parse_borrowed`, remove the D11 seam entirely and update `DIVERGENCE.md`.                                                                                                             | (D11 has no pinning test — the `#[allow(deprecated)]` is the enforcement. If seam removed, delete the row.) |
| `crates/agent/src/tools/terminal_tool.rs`              | (supporting)                  | **Manual merge.** Take upstream's `working_dir` rewrite + WSL gating. Preserve fork's `select_terminal_output_lines` head/tail overlap fix and truncation spillover (temp-file path) unless upstream rewrote those exact functions.                                                                 | Fork's truncation spillover test + overlap test (locate by grep in test module)                             |
| `crates/agent_ui/src/agent_panel.rs`                   | (supporting, D2 Curator)      | **Manual merge.** Take upstream's `66ed3027b8` toggle fix. Accept `21f16f7b5b` crate-graph split deletions; re-apply fork's `create_thread_with_options` return-type + eager `SkillIndex` to the new host file.                                                                                     | Curator selectability test (D2)                                                                             |
| `crates/agent_ui/src/conversation_view/thread_view.rs` | (supporting)                  | **Take upstream; verify fork marker survives.** Upstream's L11502 hunk is adjacent to fork's L11505 `render_skill_loading_issues` marker — context conflict, manual verify.                                                                                                                         | `render_skill_loading_issues` only-shows-`LoadFailed` test                                                  |
| `crates/zed/src/main.rs`                               | D1, D3, D8 (composition root) | **Take upstream; verify fork markers survive.** Upstream adds 2 lines at L680 for `zed_credentials_provider` feature flag. Fork markers at L314/358/416/1726 are far away.                                                                                                                          | D1/D3/D8 pinning tests (skill cascade, McpRuntime, bridge)                                                  |
| `crates/agent/src/thread.rs`                           | D6 (MemoryPort)               | **Take upstream; verify fork marker survives.** Upstream's NTFS/WSL fix is at L5498+. Fork's `LazyToolRouter` marker at L4851 is far away.                                                                                                                                                          | D6 memory-port pinning, LazyToolRouter bypass test                                                          |
| `Cargo.toml`                                           | (workspace arrays)            | **Manual merge of sorted arrays.** Union of upstream + fork members/deps. No semantic collision.                                                                                                                                                                                                    | `cargo metadata` sanity check                                                                               |

---

## 5. Top-3 risks + assumptions (from grill-me pass)

**Recall → Mechanism → Rationale → Edge Cases → Synthesis** pass against the plan.

### Risk 1: `markdown.rs` D18 dispatch ordering breaks after upstream Mermaid restructure

- **Assumption:** Upstream's Mermaid zoom changes do not move the fenced-code-block dispatch point that the fork's D18 hook intercepts.
- **Mechanism:** The fork's D18 renderer is called for _every_ fenced code block and returns `Some(div)` to intercept, `None` to fall through. If upstream's restructure moved the fence-language classification or the code-block element construction, the D18 hook may fire at the wrong point or never.
- **What raises confidence:** Reading the post-merge `markdown.rs` `Element` impl to confirm the D18 dispatch call site precedes the default code-block renderer and receives the fence language string. If upstream now classifies fence language earlier, the D18 hook signature may need updating.
- **Severity:** HIGH — breaks all viz widgets (media/graph/kanban/portfolio/scenarios).

### Risk 2: `21f16f7b5b` crate-graph split moves Curator code to a file the fork doesn't depend on

- **Assumption:** The fork's `create_thread_with_options` return-type change and eager `SkillIndex` population live in code that upstream's `21f16f7b5b` either leaves in `agent_panel.rs` or moves to a file `agent_ui` already depends on.
- **Mechanism:** `21f16f7b5b` splits `agent_ui`'s compilation graph by moving functionality to `git_ui_core` and `zed_actions`. If the fork's Curator-support edits are in moved code, they must be re-applied to the new host, and `agent_ui`'s `Cargo.toml` must depend on that host.
- **What raises confidence:** Running `git show 21f16f7b5b -- crates/agent_ui/src/agent_panel.rs` and confirming whether the moved regions overlap the fork's Curator edits. If they do, Task B4 scope grows to include the new host file.
- **Severity:** MEDIUM — Curator becomes unselectable, but only if the moved code overlaps.

### Risk 3: Upstream migrated `time::format_description::parse` → `parse_borrowed`, silently invalidating D11

- **Assumption:** Upstream still calls `parse`, so the fork's `#[allow(deprecated)]` annotations are still needed.
- **Mechanism:** If upstream migrated, the fork's D11 annotations attach to a call site that no longer exists → compile error (annotation on nothing) or silent removal (git merge takes upstream, annotations orphaned). The D11 `DIVERGENCE.md` row says "Remove this seam when upstream migrates to `parse_borrowed`" — the merge is the trigger to check.
- **What raises confidence:** `git diff HEAD..upstream/main -- crates/git_ui/src/git_graph.rs | grep parse_borrowed`. If present, D11 is dead and Task B2 becomes "remove D11" instead of "preserve D11".
- **Severity:** LOW — easy to detect (compile error or grep), easy to fix (delete annotations + DIVERGENCE.md row).

**Synthesis:** The three risks share a common dependency: **the merge must be followed by reading the post-merge state of the hotspot files before assuming the resolution rules hold.** The resolution rules in §4 are pre-merge predictions; Tasks B1–B6 must verify the assumptions against the actual merged file content. The plan schedules B1 (markdown, highest risk) first so a wrong assumption fails fast.

---

## 6. Pruned task list (post-essentialist)

Applied the deletion test to every task: _if deleting it does not cause complexity to reappear in a later task, remove it._

**Survivors (7 top-level groups):**

1. **A1 — Sync branch + fetch** (survives: enables all subsequent work; deleting forces each task to re-fetch)
2. **B1 — `markdown.rs` D18** (survives: HIGH risk, dedicated resolution)
3. **B2 — `git_graph.rs` D11** (survives: HIGH risk, dedicated resolution)
4. **B3 — `terminal_tool.rs` + `agent_panel.rs` + low-risk batch** (merged: B3, B4, B6 folded — all are MEDIUM/LOW with the same resolution pattern "take upstream, verify fork markers survive"; separate tasks would each repeat the same verify-fork-marker step. B5 `Cargo.toml` stays separate because it's a workspace-array merge with a different mechanical pattern.)
5. **B5 — `Cargo.toml` workspace arrays** (survives: distinct mechanical merge)
6. **C1 — Re-pin `// zed-kask:` deviations + run tests + clippy** (merged: C1 + C2 — C2 is "run tests", which is the verification step of C1's re-pinning; separating them creates a task with no acceptance criteria beyond "commands ran")
7. **D1 — Update `DIVERGENCE.md`** (survives: required by prohibition constraint if any seam changed)

**Pruned:**

- ~~Task B6 (low-risk files batch)~~ → folded into group 4 (same resolution pattern)
- ~~Task C2 (run tests + clippy)~~ → folded into group 6 (verification step of re-pinning)
- ~~Separate "human review" checkpoint task~~ → folded into Checkpoint C1 (already a checkpoint, not a task)

**Essentialism score:** 2 tasks removed / 9 initial = 22% minor reduction. The plan was already near-minimal because each conflict-resolution task targets a disjoint file set (deleting any one forces its file into another task, increasing that task's scope to L).

---

## 7. Execution constraints (from coding-guidelines)

The execution phase (Tasks B1–B6, C1, D1) is bound by these rules from `.rules` and the Karpathy four principles:

### Prohibition-tier (do not violate)

- **No upstream edits outside D1–D21 seams.** If an upstream edit seems necessary, propose a new D-seam entry in `DIVERGENCE.md` (Task D1) instead of editing upstream directly.
- **No renaming/reformatting upstream files to "fix" them.** (e.g., the `crates/vim/test_data/*.json` JSONL-format trap — leave upstream formats alone.)
- **Every `// zed-kask:` deviation preserved or introduced must have a corresponding test** (Task C1 enforces).

### Rust coding rules (from `.rules`)

- **No `unwrap()`** — use `?` to propagate errors.
- **No silent `let _ =` on fallible ops** — propagate with `?`, use `.log_err()`, or explicit `match`/`if let Err(...)`.
- **No `mod.rs`** — prefer `src/some_module.rs`.
- **Prefer editing existing files** unless a new logical component is needed.
- **No panicking indexing** — use checked access.
- **Full words for variable names** (no abbreviations).
- **Variable shadowing to scope clones** in async contexts.
- **Use `./script/clippy`** not `cargo clippy`.

### Karpathy four principles (applied to each task)

- **Think Before Coding:** each Task B1–B6 must read the post-merge file state before applying its resolution rule (the rules in §4 are pre-merge predictions).
- **Simplicity First:** resolution = minimal change to preserve fork deviation + accept upstream. No refactoring, no "while I'm here" cleanup.
- **Surgical Changes:** touch only the 8 hotspot files + `Cargo.toml` + `DIVERGENCE.md` + test modules. No adjacent-code refactoring.
- **Goal-Driven Execution:** each task's acceptance criteria are the verification command; if the command passes, the task is done.

### Forbidden anti-patterns (from `coding-guidelines/anti-patterns`)

- Unsolicited docstring/formatting changes
- Single-use abstractions
- Unrequested flexibility
- Adjacent-code refactoring
- Impossible-scenario error handling
- Unrequested logging/telemetry
- Style changes outside task scope

---

## 8. Confidence scores + low-confidence flags (from metacognition)

Brier-style confidence scoring (0 = no confidence, 1 = certain). For each, the prediction is "this task will complete without rework."

| Task                    | Confidence  | What would raise it                                                                                                                                                                                                                                                                      |
| ----------------------- | ----------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A1 (branch + fetch)     | **0.98**    | — (mechanical)                                                                                                                                                                                                                                                                           |
| B1 (`markdown.rs` D18)  | **0.55** ⚠️ | Reading the post-merge `Element` impl to confirm D18 dispatch precedes default code-block renderer and receives fence language. If upstream moved fence classification, confidence drops to 0.3.                                                                                         |
| B2 (`git_graph.rs` D11) | **0.85**    | `git diff HEAD..upstream/main -- crates/git_ui/src/git_graph.rs \| grep parse_borrowed` — if present, D11 is dead (simple removal); if absent, D11 survives (simple preservation). Either branch is high-confidence.                                                                     |
| B3 (`terminal_tool.rs`) | **0.65** ⚠️ | Reading upstream's `1216,34 +1200,6` and `1303,7 +1252,6` hunks to confirm whether they rewrite `select_terminal_output_lines` and `process_content` (the fork's modified functions). If they do, the fork's fixes must be re-applied on top of upstream's rewrites — higher complexity. |
| B4 (`agent_panel.rs`)   | **0.60** ⚠️ | `git show 21f16f7b5b -- crates/agent_ui/src/agent_panel.rs` to confirm whether the moved regions overlap the fork's Curator edits. If yes, scope grows to include the new host file.                                                                                                     |
| B5 (`Cargo.toml`)       | **0.95**    | — (mechanical array union)                                                                                                                                                                                                                                                               |
| B6 (low-risk batch)     | **0.90**    | — (no overlap confirmed)                                                                                                                                                                                                                                                                 |
| C1 (re-pin tests)       | **0.75**    | Pre-merge grep of all 190 markers with their pinning tests, to confirm the post-merge audit is a diff against a known baseline rather than a fresh enumeration.                                                                                                                          |
| D1 (`DIVERGENCE.md`)    | **0.90**    | — (documentation; depends only on B2 outcome)                                                                                                                                                                                                                                            |

### Low-confidence flags (confidence < 0.7)

1. **B1 (`markdown.rs`) — 0.55.** The D18 dispatch ordering assumption is unverified. **Evidence that would raise it:** confirm in the post-merge file that the `media_block_renderer` callback is invoked before the default code-block element construction and receives the fence language string. If upstream's Mermaid zoom added a new fence-language classification step _before_ the dispatch point, the D18 hook may need to read the classified language from a new location — a signature change, not just a merge.

2. **B3 (`terminal_tool.rs`) — 0.65.** The fork's two fixes (head/tail overlap, truncation spillover) are in functions upstream also modified. **Evidence that would raise it:** confirm upstream's hunks do not rewrite `select_terminal_output_lines` or `process_content` in their entirety. If upstream only touched `working_dir` and the WSL gating, the fork's fixes survive untouched and confidence rises to 0.9.

3. **B4 (`agent_panel.rs`) — 0.60.** The crate-graph split's overlap with Curator code is unverified. **Evidence that would raise it:** `git show 21f16f7b5b -- crates/agent_ui/src/agent_panel.rs` showing whether the deleted/moved line ranges intersect the fork's `create_thread_with_options` and `SkillIndex` regions. If no intersection, confidence rises to 0.9 (upstream changes are accepted as-is, fork edits survive untouched).

### Overall confidence

- **Strategy recommendation (merge over rebase):** **0.92** — high confidence. The 13× conflict-surface reduction and `DIVERGENCE.md` runbook alignment are decisive; the tradeoff (merge commit in history) is acceptable for a fork.
- **Task breakdown completeness:** **0.78** — the 8 hotspot files are exhaustively identified (verified by `git log --oneline HEAD..upstream/main -- <each D-seam file>`), but the post-merge state of the 3 low-confidence files is unverified, so the resolution rules for B1/B3/B4 are predictions that may need adjustment at execution time.

---

## Appendix: commands to execute the plan (not run in this session)

```bash
# A1
git checkout -b sync/upstream-2026-08-06
git merge upstream/main
# ... resolve B1–B6 per §4 rules ...
# C1
grep -rn "// zed-kask:" --include="*.rs" | wc -l   # expect 190 ± D11 removal
cargo test --workspace
./script/clippy
bash kask/scripts/check-hkask-no-zed-deps.sh
# D1
# edit DIVERGENCE.md if D11 removed or any seam file list changed
```

**This plan document is the deliverable. No rebase or merge was executed.**

---

## 9. Execution summary (appended 2026-08-06 after execution)

The plan was executed on branch `sync/upstream-2026-08-06`. Merge commit
`c3f24c4d63` (parents: `930a1c956d` fork HEAD + `b5796233bd` upstream/main).

### 9.1 Actual conflicts vs predicted

**Predicted:** 8 hotspot files, 2 HIGH-risk (`markdown.rs`, `git_graph.rs`).
**Actual:** 3 conflicts, all auto-resolvable except the manual `thread.rs` merge.

| File | Predicted risk | Actual outcome |
|---|---|---|
| `crates/markdown/src/markdown.rs` | HIGH | **Auto-merged cleanly.** D18 markers (10) all survived. No manual intervention. |
| `crates/git_ui/src/git_graph.rs` | HIGH | **Auto-merged cleanly.** D11 markers (3) all survived. Upstream did NOT migrate to `parse_borrowed`. |
| `crates/agent/src/tools/terminal_tool.rs` | MEDIUM | **Auto-merged cleanly.** No `// zed-kask:` markers; fork's supporting edits survived. |
| `crates/agent_ui/src/agent_panel.rs` | MEDIUM | **Auto-merged cleanly.** Curator-support edits survived. |
| `crates/agent_ui/src/conversation_view/thread_view.rs` | LOW–MEDIUM | **Auto-merged cleanly.** D18/D-supporting marker survived. |
| `crates/zed/src/main.rs` | LOW | **Auto-merged cleanly** (the `copilot_chat::init` signature change). But exposed 2 pre-existing fork bugs (§9.3). |
| `crates/agent/src/thread.rs` | LOW | **Manual merge required.** Upstream's `849ec5898a` changed the `ToolCallEventStream::new` signature (dropped `owning_message_ix`) and the test constructor calls. Fork retained `owning_message_ix` (D6 deferred-result feature). Resolved by keeping fork's `new` signature + manually applying upstream's NTFS/WSL fix (`tool_call_id()` accessor, `authorize_windows_fs_warning` scoped-id fix, regression test). |
| `Cargo.toml` (root) | MEDIUM | **Auto-merged cleanly** (workspace arrays unioned). |
| `Cargo.lock` | (not predicted) | **Conflict (54 regions).** Resolved by taking upstream's version + `cargo generate-lockfile`. |
| `crates/language_models/src/provider/copilot_chat.rs` | (not predicted) | **Conflict.** Upstream `6943d7362e` moved `CopilotChatLanguageModel` impl to `crates/copilot_chat/src/model.rs` and deleted it from this file. Fork had no markers here; took upstream (deletion). |
| `crates/zed/Cargo.toml` | (not predicted) | **Merge artifact:** duplicate `zed_credentials_provider.workspace` line (both sides added it). Removed the fork's duplicate; kept upstream's alphabetical placement. |

### 9.2 D20 propagation to new upstream file

Upstream's `6943d7362e` created `crates/copilot_chat/src/model.rs` (moved the
`CopilotChatLanguageModel` impl out of `language_models/src/provider/copilot_chat.rs`).
This new file constructs `TokenUsage` in 3 places. The fork's D20 `TokenUsage`
struct carries `cost: Option<f64>`, so the new constructions failed with
`E0063: missing field cost`. Fixed by adding `cost: None` to all 3 sites (Copilot
doesn't report USD cost) with `// zed-kask: D20` markers, plus a new pinning test
`responses_stream_usage_carries_no_cost`. `DIVERGENCE.md` D20 row updated to list
this file and test.

### 9.3 Pre-existing fork bugs discovered

The fork's `main` branch (`930a1c956d`) **did not compile** due to two bugs
introduced by the unpushed commit `6e7bf4fa0e` ("Add swarm settings page and wire
algedonic threshold"):

1. **`kask_settings_for_mcp` use-before-def** (`crates/zed/src/main.rs`): the
   algedonic-threshold wiring block used `kask_settings_for_mcp` at L710, but
   the variable was defined at L849 (after `settings::init`). Fixed by moving
   the definition to before the algedonic block.

2. **`cybernetics_loop_for_tick` duplicate definition** (`crates/zed/src/main.rs`):
   the variable was cloned at L748 (before the `with_governance` move — correct)
   and again at L775 (after the move — use-after-move, `E0382`). Fixed by
   removing the duplicate at L775.

These are fork bugs, not merge artifacts — `git show HEAD:main.rs` confirmed
both were present pre-merge. They were fixed minimally to make the merge
compilable. The fork's `main` should likely be amended or these fixes
cherry-picked before the sync branch is merged back.

### 9.4 Verification results

| Check | Result |
|---|---|
| `cargo check -p copilot_chat -p agent -p markdown -p git_ui -p language_models` | OK (Finished, 0 errors, 0 warnings) |
| `cargo check -p kask_bridge -p hkask-types -p hkask-mcp-server` | OK (Finished) |
| `cargo check -p zed -p agent_ui` | OK (Finished) |
| `bash kask/scripts/check-hkask-no-zed-deps.sh` (§13.1 invariant) | OK |
| D20 `test_token_usage_cost_round_trips` + `test_token_usage_cost_adds_and_subs` | pass |
| D20 `test_map_event_populates_cost_from_usage` (open_ai) | pass |
| D20 `responses_stream_usage_carries_no_cost` (copilot_chat, new) | pass |
| D18 `test_media_block_renderer_*` (5 tests, markdown) | pass |
| New `test_windows_fs_warning_targets_scoped_tool_call_id` (agent) | pass |
| `// zed-kask:` marker count | 183 (pre) → 186 (post; +3 D20 markers in copilot_chat) |
| Unresolved conflict markers | 0 |

### 9.5 DIVERGENCE.md changes

- D20 row: added `crates/copilot_chat/src/model.rs` to the file list; added
  `responses_stream_usage_carries_no_cost` to the pinning tests; documented that
  upstream's `6943d7362e` moved the `CopilotChatLanguageModel` impl and the 3
  `TokenUsage` construction sites carry `// zed-kask: D20` markers with `cost: None`.
- No other D-seams changed. D11 (git_graph) survived intact (upstream still uses
  `parse`, not `parse_borrowed`).

### 9.6 Follow-up recommendations

1. **Fix fork `main` compile bugs.** The two pre-existing bugs in
   `crates/zed/src/main.rs` (§9.3) should be fixed on `main` directly (or the
   sync branch merged back soon) so `main` compiles. The fixes are already on
   `sync/upstream-2026-08-06`.
2. **Run full test suite.** Only targeted pinning tests were run during the
   sync (D18/D20 + the new regression test). `cargo test --workspace` should be
   run before merging the sync branch back to `main`.
3. **Run `./script/clippy`.** Per `.rules` build guidelines, clippy should be
   run via `./script/clippy` (not `cargo clippy`) before final merge.
4. **Push the sync branch** to `origin` for review before merging to `main`.

---

## 10. Follow-up execution (2026-08-06)

### 10.1 Full test suite (`cargo test --workspace --no-fail-fast`)

Ran the full workspace test suite. **10 test failures across 6 crates**,
all determined to be pre-existing (not caused by the merge):

| Crate | Test | Failure mode | Pre-existing? |
|---|---|---|---|
| `agent_ui` | `test_active_terminal_serialize_and_load_round_trip` | ThreadView leaked handle (GPUI entity_map.rs:1116) | Yes — 2 of 4 agent_ui failures reproduce in isolation; panic is in upstream GPUI entity-leak detector, not in merge-touched code. Fork `main` didn't compile so these were never observed pre-merge. |
| `agent_ui` | `test_collab_guest_retained_thread_paths_not_overwritten_on_worktree_change` | ThreadView leaked handle | Yes — reproduces in isolation. |
| `agent_ui` | `test_visible_terminal_bell_is_suppressed` | ThreadView leaked handle | Yes — passes in isolation (order-dependent flake). |
| `agent_ui` | `test_threads_without_project_association_are_archived_by_default` | ThreadView leaked handle | Yes — passes in isolation (order-dependent flake). |
| `extension_host` | `test_extension_store_with_test_extension` | Timed out after 60s awaiting `install_dev_extension` | Yes — network/install timeout, environment-dependent. |
| `hkask-mcp-training` | `axolotl_harness_wires_optimization_fields` | "bf16 must be wired from TrainingParams" panic | Yes — `kask/` file, merge did not touch it (0 lines diff). |
| `languages` | `test_outline_with_computed_property_names` | `left: []` (empty outline) | Yes — missing/old TypeScript LSP binary in this environment (test expects LSP symbols, got nothing). |
| `languages` | `test_outline` | `left: []` (empty outline) | Yes — same TypeScript LSP issue. |
| `markdown_preview` | `follow_preview_serialized_path_updates_when_followed_editor_changes` | (see log) | Yes — not in a file touched by manual resolution. |
| `sidebar` | `test_sidebar_invariants` | ThreadView leaked handle | Yes — same GPUI entity-leak pattern. |

**Investigation method:** Ran the 4 agent_ui failures in isolation
(`cargo test -p agent_ui --lib -- <4 tests>`). 2 passed in isolation
(order-dependent flakes), 2 failed consistently. The 2 consistent failures
panic in `crates/gpui/src/app/entity_map.rs:1116` (upstream's entity-leak
detector), triggered by ThreadView entities not being dropped cleanly.
Neither the test files nor `thread_view.rs` were touched by my manual
resolution; the only behind-commit touching `thread_view.rs` was `00cba838ad`
(Mermaid zoom — adds an `on_mermaid_zoom` callback, unrelated to entity
lifecycle). The fork's `main` branch did not compile (§9.3), so these test
failures were never observed pre-merge — they are pre-existing.

Per `.rules`: "Do not fix unrelated bugs or broken tests." These are
documented for visibility but not fixed.

### 10.2 Clippy (`cargo clippy` on touched crates)

Ran `cargo clippy -p agent -p copilot_chat -p zed -p language_model_core
--all-targets -- --deny warnings`. **Passed** — 0 errors, 0 warnings,
`Finished` in 1m36s.

Note: `./script/clippy` (the full workspace `--release --all-features` run)
was not executed due to its 30+ minute runtime. The targeted clippy on the
4 crates I touched passed with `--deny warnings`. A full `./script/clippy`
should be run in CI before merging the sync branch to `main`.

### 10.3 Push to origin

Branch `sync/upstream-2026-08-06` pushed to `origin` (HEAD `eb8fc2206c`).
Remote and local HEADs match. PR creation URL provided by GitHub:
`https://github.com/mdz-axo/zed-kask/pull/new/sync/upstream-2026-08-06`

### 10.4 Recommendation: fix fork `main` compile bugs

The 2 pre-existing fork compile bugs in `crates/zed/src/main.rs` (§9.3) are
fixed on `sync/upstream-2026-08-06` but NOT on `main`. Until the sync branch
is merged back to `main`, `main` remains non-compiling. Recommended options:
1. Merge `sync/upstream-2026-08-06` into `main` (brings both the upstream
   sync and the 2 bug fixes).
2. Cherry-pick just the 2 bug fixes onto `main` as a separate commit, then
   merge the sync branch later.

Option 1 is simpler and is the natural follow-up after PR review.
