---
name: upstream-rebase
description: "Manage upstream Zed merges for zed-kask. Decides per D-seam whether its user-visible purpose still needs fork divergence (retire, simplify, retain), then per retained file whether to git-merge or mapped-re-apply, verifies the result against both parents, and updates DIVERGENCE.md."
---

# Upstream Rebase

Manage upstream Zed rebases for zed-kask, preserving the fork's functional
kask-wiring changes without carrying forward accumulated cruft.

## When to Use

- When bringing zed-kask up to upstream Zed's `main` (`git fetch upstream && git merge upstream/main`).
- When a D-seam file has accumulated divergence (under-marked `// zed-kask:` markers, compile bugs from incremental evolution, > 2× upstream line count).
- When auditing a D-seam file's marker density and pinning test coverage after a merge.
- When you need to decide, per file, whether to git-merge or mapped-re-apply.

## When NOT to Use

- Fresh feature work with no upstream merge in flight — the per-file strategy decision exists only during a rebase.
- A fork with no `DIVERGENCE.md` seam record — there is nothing to map or re-apply.
- Deletion D-seams through Steps 1–7 — they are not files; skip to Step 8 (post-rebase cleanup) for them.

## The three strategies

| Strategy                  | When to use                                                                                   | Risk         | Effort |
| ------------------------- | --------------------------------------------------------------------------------------------- | ------------ | ------ |
| **Git merge**             | File is well-marked (every deviation has `// zed-kask:` + test) and compiles cleanly          | Low          | Low    |
| **Mapped re-application** | File is under-marked (< 50% of kask call sites carry markers), has cruft, or has compile bugs | Medium       | High   |
| **Destroy and rebuild**   | Never                                                                                         | Catastrophic | High   |

**Decision rule (`UR-1`):** if the fork's file has > 2× the upstream line count, or < 50% of kask call sites carry `// zed-kask:` markers, use mapped re-application. Otherwise use git merge. Gather the four counts via shell (`wc -l <target_file>`, `git show <upstream_ref>:<target_file> | wc -l`, `grep -c "kask_bridge::\|hkask_\|hkask-" <target_file>`, `grep -c "// zed-kask:" <target_file>`) and compute `assessment_result` in `lisp_eval` `UR-1` — never by hand:

```
form: (let ((dens (if (= sites 0) 1 (/ markers sites)))) (list dens (if (or (> fork (* 2 upstream)) (< dens 0.5)) (quote mapped-reapplication) (quote git-merge))))
env:  { "fork": <fork lines>, "upstream": <upstream lines>, "markers": <marker count>, "sites": <kask call sites> }
```

Tested cases: 100/100/5/10 → git-merge; 100/100/4/10 → mapped-reapplication; 201/100/10/10 → mapped-reapplication; 50/100/0/0 → git-merge; 250/100/0/0 → mapped-reapplication.

## Reference model

There is no external reference model. The per-seam retire / simplify / retain decision and the per-file git-merge vs mapped-re-apply rule are this repository's own practice, derived from `DIVERGENCE.md`'s seam ledger and the `// zed-kask:` marker convention; the oracles are `git`, `cargo check`/`cargo test` and the isolation scripts, not a published merge methodology. Treat the strategy table as a house rule that has held across the recorded rebases, open to revision when a rebase falsifies it.

## Initial and target condition

- **Initial condition:** the merge base, `git diff --name-only <base> upstream/main`, and the live D-rows in `DIVERGENCE.md`.
- **Target condition:** a merge commit on which Verification gate items 0–5 pass and every live D-row has a recorded retire / simplify / retain / needs-operator-decision decision.

## Step types (D/P labelling)

| Step | Type | Oracle / critique |
|------|------|-------------------|
| 0 Survival, 2 Constraint force, 3 Dependency graph | P | the operator on NEEDS OPERATOR DECISION; each record's counterexample; `UR-2` `lisp_eval` inventory gate |
| 1 Inventory, 4 Insertion points, strategy rule (`UR-1`) | D | `git`, `grep`, `lisp_eval` (`UR-1`, `UR-2`) |
| 5 Re-apply | P | the Verification gate + `UR-3` `lisp_eval` |
| 6 Pins, 8 Cleanup, Verification gate (`UR-3`) | D | `cargo check`/`cargo test`, the isolation scripts, `UR-3` `lisp_eval` |
| 7 DIVERGENCE.md update | D | grep: the row's named file paths and pin tests exist in the tree |
| 9 Reflect | P | the operator accepts or rejects each amendment |

## Instructions

### Step 0 — Decide whether each affected seam should survive

Before choosing a merge strategy, review **every live D-row**, not only rows
whose files upstream changed. A seam can go stale without any upstream edit:
the operator can deprecate it, or its premise (a dependency version, a bug) can
stop being true. Two tiers:

- **Upstream-touched rows** (`git diff --name-only <base> upstream/main`
  intersected with the row's file column): a full decision record below.
- **Untouched rows**: a short check. Is the purpose still wanted under the
  operator's current requirements? Is the row's stated premise still true in
  the tree (versions, callers, the upstream bug it fixes)? For a version
  premise, quote the row next to `grep -A1 'name = "<crate>"' Cargo.lock`;
  for an upstream bug, cite the upstream function that still has it. Do its cited pins
  still exist? Any "no" promotes the row to a full record.

The full record:

- **Purpose** — the user-visible outcome the seam protects and the failure
  the user sees without it.
- **Current authority** — the operator's current requirement, not the row's
  history. Sources: `.rules`, operator rulings recorded in `DIVERGENCE.md`,
  and `curator_semantic_search` for deprecations; if none settles it, the
  decision is NEEDS OPERATOR DECISION. A row describing a capability the operator has since deprecated
  (e.g. a token budget after the max-token deprecation) is not authority.
- **Upstream evidence** — the specific function at the new upstream tip that
  does or does not deliver the purpose. A same-named function, a clean
  conflict resolution, or a green compile is not equivalence.
- **Counterexample** — one input where upstream and fork behavior differ.
- **Decision** — RETIRE (upstream delivers it, or the operator deprecated it),
  SIMPLIFY (keep only the part upstream still lacks), RETAIN, or NEEDS
  OPERATOR DECISION (the requirement itself is ambiguous; ask in functional
  terms). Do not default to RETAIN.

RETIRE removes the coupled code, settings, tests, dependencies, comments, and
D-row in the same merge, and adds the number to `DIVERGENCE.md`'s retired-seam
list (numbers are never reused). Only surviving seams continue to Steps 1–7.

**Scope:** Steps 1–7 apply to D-seam *files* — rows whose `DIVERGENCE.md` file
column names an existing file. Deletion D-seams (file column `—` or
~~struck through~~ — check the retired-seams list for current examples) are not files; skip Steps 1–7 for them and
proceed to Step 8 to verify the merge did not silently restore them.

### Step 1 — Establish the functional inventory (code-graph extraction)

Extract every kask-wiring functional unit from the fork's file. A _functional unit_ is a contiguous block implementing one kask capability. Extract manually with `git diff upstream/main HEAD -- <file>` (to see what the fork added) plus `grep` for section headers and kask symbols.

Output: a numbered list of functional units (F1, F2, …) with line ranges and one-sentence purpose.

### Step 2 — Classify each unit by constraint force (semantic-mode audit)

Classify every functional unit by pragmatic-semantics constraint force:

- **Prohibition** — must re-apply or the system breaks (load-bearing wirings).
- **Guardrail** — should re-apply; omitting degrades behavior but doesn't break.
- **Guideline** — nice-to-have; omitting is a regression but not a failure.
- **Evidence** — diagnostic/observability.
- **Hypothesis** — speculative/future-facing.

Output: a table mapping each unit to its constraint force + enforcement point + pinning test (or "not yet pinned").

### Step 3 — Build the dependency graph (ordering constraints)

For each functional unit, identify what it _defines_ and what it _uses_. This produces a DAG. The re-application order must be a topological sort — this is where use-before-def bugs come from: incremental fork evolution violates the DAG by inserting a use before its definition.

Output: a dependency table (unit → defines → uses → must-come-after).

Verify the extracted `inventory_result` (`map.j2` output) with `lisp_eval` gate `UR-2` before proceeding to `decide.j2` — every unit in `functional_units` must appear in `topological_order` and every `"Prohibition"` unit in `constraint_force_table` must have a non-empty `enforcement_point`:
```
form: (begin (define proh-ok (lambda (rows) (if (is_null rows) true (let ((r (car rows))) (if (string= (assoc "force" r) "Prohibition") (and (not (string= (assoc "enforcement_point" r) "")) (proh-ok (cdr rows))) (proh-ok (cdr rows))))))) (define all-in (lambda (xs ys) (if (is_null xs) true (and (member (car xs) ys) (all-in (cdr xs) ys))))) (and (= (length functional_units) (length topological_order)) (> (length functional_units) 0) (all-in functional_units topological_order) (all-in topological_order functional_units) (proh-ok constraint_force_table)))
env:  { "functional_units": <functional_units>, "topological_order": <topological_order>, "constraint_force_table": <constraint_force_table> }
```

### Step 4 — Map insertion points in clean upstream

For each functional unit, identify the insertion point in clean upstream's file — the landmark line after which the unit should be inserted.

Output: a table mapping each unit to its upstream insertion landmark.

### Step 5 — Re-apply (manual editing)

Take clean upstream's file and insert each functional unit at its mapped insertion point, in topological order. For each insertion:

- Add a `// zed-kask: D<N>` marker pointing to the DIVERGENCE.md row.
- Ensure `let` bindings are placed before any use.
- Ensure no duplicate definitions.

### Step 6 — Pin surviving behavior at the smallest meaningful boundary

Each retained or simplified seam needs one behavior test that fails when its
purpose is lost. Put it at the cheapest boundary that exercises that purpose:
prefer a `kask/` crate or an existing fork-owned test module over adding a test
to an upstream-owned Zed file. A seam retired in Step 0 takes its pins with it;
an obsolete pin (one that tests a replaced upstream API rather than the
purpose) is removed, not ported. If the only boundary is an upstream-owned file,
record "pinned by review only" in the row and ask the operator before adding a
test there. For process-global hooks (e.g., `main.rs`
wirings), a compile-time + symbol-existence pin is acceptable.

### Step 7 — Update DIVERGENCE.md

Update the D-seam row to reflect the re-applied file: list the file, document every functional unit's constraint force and pinning test.

### Step 8 — Run post-rebase cleanup

`git merge upstream/main` can restore files zed-kask deliberately deleted under
D7/D16 (icons, `.desktop` templates, `script/bundle-mac`, Flatpak/Snap resources,
release workflows). `kask/scripts/build/check-zed-isolation.sh` is the enforcement
point — it enumerates every forbidden path and is wired into CI
(`.github/workflows/kask-invariants.yml`). Running it locally
closes a fast loop (seconds) instead of waiting for the CI round-trip.

1. `bash kask/scripts/build/check-zed-isolation.sh`
2. If it fails, it names the offending path — re-delete that path and re-run.
3. Repeat until it passes. Bound: max 3 sweep rounds; a fourth failure means
   the merge restored something structural — halt and report instead of
   re-deleting.

Do not re-list the forbidden paths here — the script is the authority and its
list updates independently of this skill.

### Step 9 — Reflect and amend this skill

After the merge commit exists and before closing the task, render
`upstream-rebase/reflect` with the merge commit, the seam decisions, and a
log of what actually happened (commands that failed, surprises, operator
corrections, time lost). It compares that log against what this skill told
the agent to do and returns:

- **Deviations** — each place the skill was wrong, silent, or ignored, with
  the evidence (commit, command, or operator message).
- **Amendments** — one concrete edit per deviation to `SKILL.md`, a template,
  or `upstream-rebase-process.md`, each tied to a falsifiable case
  ("on merge N, an agent following the old text would do X; the new text makes
  it do Y").
- **Held-out case** — the case from this merge the amended skill must pass on
  the next sync.

Present the amendments to the operator; apply only the accepted ones, in a
commit separate from the merge. Record the held-out cases in
`kask/docs/reference/upstream-rebase-process.md` §10 so the next sync starts
by checking them. A lesson that no case can falsify is not an amendment.

## Verification gate (before committing)

0. Compare the merged tree against **both** parents. For every path changed on
   both sides, the merged blob must not equal the fork parent's blob unless
   Step 0 retired upstream's change deliberately, and upstream-added functions
   and tests must be present or accounted for (renamed, retired, or replaced).
   `-X ours`, `checkout --ours`, and marker-free indexes can silently drop
   upstream work. Mechanical procedure, upstream side:
   `git diff --name-only <base> <upstream> | sort` minus
   `git diff --name-only <base> <fork-head> | sort` (`comm -23`) gives the
   upstream-only files; `git diff <upstream> -- <that list>` (against the
   WORKING TREE, never `HEAD`, which mid-merge still names the pre-merge
   commit — observed 2026-10-27: against HEAD every one of 409 files read as
   differing; against the worktree, 3) must be
   empty except files carrying a recorded re-home (2026-10-12: 587/588
   identical; the one exception the D89 re-home). The both-changed remainder
   is checked against the Step 0 records file-by-file.
1. `cargo check -p <crate>` — the file compiles.
2. `cargo test -p <crate> -- <pinning tests>` — all pinning tests pass.
3. `bash kask/scripts/check-hkask-no-zed-deps.sh` — §13.1 invariant holds.
4. `grep -c "// zed-kask:" <file>` — marker count matches the functional unit count.
5. `git diff upstream/main -- <file>` — no upstream line is modified except under a live D-row (retire/simplify work may remove fork lines).
6. Run `lisp_eval` gate `UR-3` over the execution and verification results (`verification_result`). `compiled`, `tests_passed`, `invariant_holds`, `marker_count`, and `call_site_count` are MEASURED by the caller — `cargo check -p <crate>`'s exit, the pinning-test run's exit, `bash kask/scripts/check-hkask-no-zed-deps.sh`'s exit, `grep -c "// zed-kask:" <file>`, and the call-site count — and passed into the gate as inputs; `execute.j2` never types them:
   - form: `(and compiled tests_passed invariant_holds (= marker_count unit_count))`
   - env: `{ "compiled": <bool>, "tests_passed": <bool>, "invariant_holds": <bool>, "marker_count": <int>, "unit_count": <length of functional_units> }`

If any gate fails, return to Step 5 for that file with the failing command's output. At most 2 returns per file; a third failure halts the merge and goes to the operator with the failing output.

## Post-merge test-fallout triage

The verification gate surfaces fallout; this section diagnoses it. Check the
failing test against these merge-recurrent classes before debugging production code:

| Class | Signature | First check |
| --- | --- | --- |
| Fake snapshot semantics | Capacity/thinking arithmetic implying default capabilities (a pre-shrink panic reporting ~1M-token windowing; `thinking_allowed` false where the fork set it true) | `fake.model()` returns a clone — capture `update_model`'s return instead of mutating after capture |
| Background-request races | A test hangs, the turn never completing | Mid-turn title generation races follow-ups on the registered fake — select requests by `CompletionIntent`, never list position |
| Dropped fork setup blocks | A fork test fails on behavior its own comment says it pins | Diff the test body against the fork parent — fake-API migration drops load-bearing setup (settings blocks, parent-side `add_tool`) |
| Merge-dedup collisions | A fork test fails while its sibling passes | Same-named helpers from both parents — check which body kept the name |
| Upstream helpers × fork environment | Scheduler determinism panics naming async-std/keychain | Upstream `sign_in` helpers persist through the fork's always-keychain provider — install a test credentials-provider global first |
| Unregistered fake instance | Empty pending state on a fresh `FakeLanguageModelProvider::default()` | Bind to the registered provider instance |

Techniques: hangs block the harness's panic output — re-run with
`--skip <hang-names>` to harvest the other panics; then run the hang isolated
with `RUST_LOG=agent=debug -- --nocapture` and read the last log lines before
hypothesizing. For any failing test, identify its parent-of-record first:
diff the test against both parents — a test byte-identical to upstream's
means the fork's code side is the suspect; a fork test's oracle is the fork
parent.

## Composed Skills

| Skill               | Role                                                                                          | When Invoked  |
| ------------------- | --------------------------------------------------------------------------------------------- | ------------- |
| `grep` + manual analysis     | Code-graph extraction (manual)                                    | Steps 1-2     |
| `essentialist`      | Deletion test: is full re-application necessary, or is surgical marking + pinning sufficient? | Before Step 5 |
| `coding-guidelines` | Surgical re-application guardrails                                                            | Step 5        |
| `task-breakdown`    | Slice the re-application into vertical tasks                                                  | Step 5        |

## Ontological Anchors

- **PKO** (Procedural Knowledge Ontology): the re-application is a Procedure (specification → execution → verification). Each functional unit is a Step; the DAG is the StepExecution order; the pinning test is the StepVerification.
- **Pragmatic-semantics**: constraint-force classification (Prohibition/Guardrail/Guideline) determines which units are load-bearing.
- **Cybernetics**: the verification gate is a feedback loop (compile → test → invariant check → marker count).

## Process Document

The full process, with the `main.rs` functional inventory (28 units), DAG, and constraint-force classification, is in `kask/docs/reference/upstream-rebase-process.md`.

## Registry Templates

| Template | Purpose |
|----------|---------|
| `map.j2` | Extract the functional inventory (F1, F2, ...), classify each unit by constraint force, and build the dependency DAG. |
| `decide.j2` | Reconcile the file's seams against Step 0's survival records (deciding fresh only when Step 0 was skipped) and apply the essentialist deletion test (G1): is full re-application necessary, or is surgical marking + pinning sufficient? Consumes `survival_records`, `assessment_result` (`UR-1`), and `inventory_result` (`map.j2`). |
| `execute.j2` | Execute the chosen strategy: add markers + pinning tests (surgical), or re-apply onto clean upstream in topological order (full re-application). |
| `document.j2` | Update DIVERGENCE.md and produce the final report. |
| `reflect.j2` | After the merge commit: compare what happened against this skill, and propose amendments each tied to a falsifiable held-out case. |

To render a template, call the `render_template` tool with the template ref (e.g., `upstream-rebase/map`) and a context object with the required variables:
- `map.j2`: `target_file`, `upstream_ref`, `assessment_result`, `task`
- `decide.j2`: `target_file`, `survival_records`, `assessment_result`, `inventory_result`, `task`
- `execute.j2`: `target_file`, `upstream_ref`, `inventory_result`, `decision_result`, `task`
- `document.j2`: `target_file`, `inventory_result`, `decision_result`, `execution_result`, `verification_result`, `task`
- `reflect.j2`: `merge_commit`, `seam_decisions`, `execution_log`, `prior_held_out_cases`, `task`

Run verification gates (cargo check/test, isolation script) with `terminal` and deterministic checks (`UR-1`, `UR-2`, `UR-3`) with `lisp_eval`. Template order: Step 0's all-rows survival review (`survival_records`), `UR-1`'s per-file shell metrics + `lisp_eval` strategy rule (`assessment_result`), and Step 4's insertion-point mapping are direct agent work; then, per surviving seam file, `map` (`inventory_result`, checked by `UR-2`) → `decide` (`decision_result`) → `execute` (`execution_result`; the caller then MEASURES `compiled`/`tests_passed`/`invariant_holds`/`marker_count`/`call_site_count` from the commands above and feeds them to the Verification gate + `UR-3`, which produce `verification_result`) → `document`, then `reflect` after the merge commit. `decide.j2` returns `strategy`; the caller derives `full_reapplication` as `(strategy == "full-reapplication")`.

## Regression case

All receipts executed live through `lisp_eval` (2026-10-01, backfill pass),
over the per-file strategy decision rule — all five documented cases
reproduce exactly:

- 100/100/5/10 → `[0.5, "git-merge"]`.
- 100/100/4/10 → `[0.4, "mapped-reapplication"]` (marker density below
  0.5).
- 201/100/10/10 → `[1.0, "mapped-reapplication"]` (fork more than twice
  upstream).
- 50/100/0/0 → `[1, "git-merge"]` (no call sites: density defaults to
  1, small fork).
- 250/100/0/0 → `[1, "mapped-reapplication"]` (no call sites, large
  fork).

The skill's forms are executed at use time, never anchored in code.

## Constraints

- Do NOT modify any upstream file outside the D-seam surface. Consult `DIVERGENCE.md`'s divergence-surface table for the current D-seam rows — the table is authoritative; do not rely on a hardcoded range label (the count drifts as seams are added). If an upstream edit seems necessary, propose a new D-seam entry in `DIVERGENCE.md`.
- Do NOT rename or reformat upstream files to "fix" them.
- Every retained or simplified seam must have a behavior test at the smallest meaningful boundary (Step 6); retired seams lose their pins.
- The re-application order must be a topological sort of the dependency DAG (no use-before-def).
- Prefer surgical marking + pinning over full re-application when the fork's file already compiles and is correctly ordered (essentialist G1: identical end state, lower risk).

## Merge & rebase protocol

### Fetch & merge strategy

The project convention is **merge, not rebase** (`DIVERGENCE.md` "Upstream-sync runbook" step 1: `git fetch upstream && git merge upstream/main`). A long-lived fork tracking
upstream `main` merges — rebasing would rewrite fork history and force-push,
breaking collaborator branches. Preserve upstream history; do not squash.

#### Upstream history rewrite (verified 2026-10-27)

If `git fetch upstream` reports a **forced update** of `upstream/main`, do NOT
merge yet. Check the merge base: `git merge-base main upstream/main`. When it
resolves to something far older than the last recorded sync's upstream tip
(read DIVERGENCE.md's Upstream-sync history), the upstream history was
rewritten and a plain merge computes against a years-old tree. Verify content
faithfulness before re-linking: find the new-history commit corresponding to
the previously merged upstream tip (match by commit subject, then compare
`git rev-parse <old-tip>^{tree}` with `git rev-parse <new-equivalent>^{tree}` —
they must be byte-identical). Re-link in two steps, committing both as
ordinary merge commits: first `git merge -s ours <new-equivalent>` — a
tree-unchanged ancestry adoption that loses nothing precisely BECAUSE the
trees are identical (never use `-s ours` against a non-identical point); then
the normal `git merge upstream/main`, whose merge base is now the content point
and whose delta is the real upstream development. Name the recovery in the
re-link commit message. Observed live 2026-10-27: base collapsed to a 2021
commit; trees matched at `93d7456688` (≡ `d140451094`); 31 conflicts after the
re-link instead of a catastrophic full-tree merge.

### Post-resolution sweeps (before the gates)

Two sweeps close failure classes the unmerged-path list cannot see (both
observed live 2026-10-27):

1. **Tree-wide marker grep.** `git add` accepts files that still contain
   conflict markers — a staged-while-conflicted file drops off the unmerged
   list and the first symptom is a `mismatched closing delimiter` compile
   error. After resolving the last unmerged path, grep the WHOLE tree, not
   the status list:
   `grep -rln '^<<<<<<< HEAD$' --include='*' . | grep -v './target\|./.git'`.
2. **Sibling symbol census.** The auto-merge can graft a fork body onto
   upstream's deleted/renamed structure, or drop a fork block that lived
   between two conflict hunks in the same file (observed: the fork's
   `KeychainCredentialsProvider` impl attached to upstream's deleted
   `DevelopmentCredentialsProvider`; a two-field construction silently
   reduced to upstream's single-arg form; an entire v1 trait impl vanished
   with no conflict marker). After resolving each conflicted file, census
   the symbols its resolution depended on — `grep -n 'fn <name>\|struct <name>'`
   over the file and its callers — and confirm every fork symbol that existed
   at the fork parent still exists. The compiler eventually catches the class,
   but late and in red-loop fragments; the census catches it reading.

### Conflict classes

The `DIVERGENCE.md` Upstream-sync runbook (step 2) names the conflict
surfaces: D-seam files, workspace `Cargo.toml` arrays, `Cargo.lock`
(regenerate from the merged manifests — do not hand-merge),
`kask/deny.toml` `allow-git` (add upstream's new git sources),
`typos.toml` (deleted under D46 — resolve by re-deleting), and
modify/delete conflicts (upstream restoring D7/D16 deletions). The table
below adds the resolution detail per class — cite the runbook's step, not
line numbers, when the doc evolves:

| Class | Resolution |
| --- | --- |
| **D-seam modify/modify** | Follow the decision rule (`SKILL.md` decision rule + Step 1 scope note). Git-merge if well-marked; mapped re-application if under-marked. |
| **Kask-additive no-conflict** | No action (runbook step 3: everything under `kask/` is additive). |
| **Workspace `Cargo.toml` arrays** | Hand-merge: keep both sides' kask members; take upstream's crate renames (a stale old-name member fails loudly at the next cargo run). Covers root `Cargo.toml` and `crates/zed/Cargo.toml`. Never drop a kask member. |
| **`Cargo.lock`** | Regenerate from the merged manifests (`cargo check --workspace` rewrites it) — never hand-merge. |
| **`kask/deny.toml` `allow-git`** | Add upstream's new git source URLs; verify with `cargo deny --config kask/deny.toml check`. |
| **`typos.toml`** | Deleted under D46 — resolve delete/modify conflicts by re-deleting. |
| **Modify/delete — upstream restores** | Run Step 8 (`check-zed-isolation.sh`); re-delete every path it names; re-run until pass. |
| **Modify/delete — upstream deletes a file zed-kask modifies** | Default (Hypothesis-tier, no instance yet): if the kask wiring is still load-bearing, re-add the file as a new D-seam row (move under `kask/` if possible). If obsolete, accept the deletion and remove the `DIV` row. Either way, add/update the pinning test in the same commit. |

### Commit hygiene

Follow `.rules` "Pull request hygiene": imperative PR title, no conventional-commit
prefix, `Release Notes:` as the final section with a blank line after the heading.
For a pure upstream sync: `- N/A`. Merge commit message: `Merge upstream/main
<upstream-sha> into zed-kask`. Do not squash — upstream's commit log is the audit
trail for what changed.

### Branch & PR strategy

**Convention (observed twice):** upstream syncs land directly on `main` as
merge commits with the message `Merge upstream/main <sha> into zed-kask` —
the 2026-09-24 sync and the 2026-10-12 merge (`8e32d2a127`) both landed this
way. Do not create a per-sync branch or PR unless the operator asks for
reviewability.

### Verification gate ordering (merge-level, before pushing the PR)

The skill's per-file gate (above) is for a single re-applied D-seam file. The
merge-level gate below runs over the whole tree. Order: cheap Prohibition-class
invariants first, then compile, then tests.

1. `bash kask/scripts/build/check-zed-isolation.sh` — Zed-isolation + desktop
   no-collision (one script).
2. `bash kask/scripts/check-hkask-no-zed-deps.sh` — §13.1 invariant (DIVERGENCE.md's
   Governing invariant (§13.1) section; the rule itself is architecture plan
   §13.1).
3. `./script/clippy` — `.rules` build guidelines: "Use `./script/clippy` instead
   of `cargo clippy`." Runs under `--deny warnings`.
4. `cargo check -p kask_bridge -p hkask-types -p hkask-mcp-server` — `DIV`
   runbook step 5 (intent is
   "verify the bridge + foundation still compile").
5. `cargo test -p <affected-crates>` — derive the crate set from the merge,
   never memorize it: the crates of every file in
   `git diff --name-only <base> <upstream-sha>` plus every seam file,
   deduplicated. Run their full suites, not only the Step 6 pins.

### Recovery

A resolved but uncommitted merge exists only in the index and worktree;
`git merge --abort` and `git reset --hard` destroy it. Before any of them:

1. Check who owns the in-flight work (`git status`, `git worktree list`). Do
   not abort or reset work you did not create.
2. Snapshot it with both parents: `git commit-tree $(git write-tree) -p HEAD -p MERGE_HEAD`
   (`git stash create` drops `MERGE_HEAD`) and pin the result under `refs/recovery/<date>/…`.
3. If it is already lost, search Zed's automatic `Checkpoint` commits and
   `git fsck --no-reflogs --unreachable` for commits whose tree differs from
   both parents, and pin any candidate before reporting what is and is not
   recoverable. A new merge is not a restoration.

- **Mid-merge, uncommitted:** `git merge --abort` only after `git rev-parse refs/recovery/<date>/…` shows the snapshot exists.
- **Merge committed, not pushed:** `git reset --hard <pre-merge-sha>` (find via
  `git reflog`, the `HEAD@{1}` before the merge).
- **Mapped re-application in progress, merge already committed:** `git checkout
  -- <file>` discards one file's in-progress re-application; `git restore
  --source upstream/main -- <file>` restarts from clean upstream.
- **Merged to `main`:** do not force-push `main`. Revert with
  `git revert -m 1 <merge-sha>` and open a follow-up PR.
