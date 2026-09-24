---
shipped: false
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

**Decision rule:** if the fork's file has > 2× the upstream line count, or < 50% of kask call sites carry `// zed-kask:` markers, use mapped re-application. Otherwise use git merge.

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
~~struck through~~, e.g. D4, D10) are not files; skip Steps 1–7 for them and
proceed to Step 8 to verify the merge did not silently restore them.

### Step 1 — Establish the functional inventory (code-graph extraction)

Extract every kask-wiring functional unit from the fork's file. A _functional unit_ is a contiguous block implementing one kask capability. Use `git diff upstream/main HEAD -- <file>` + `grep` to extract manually via `git diff upstream/main HEAD -- <file>` + `grep` for section headers and kask symbols.

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
point — it enumerates every forbidden path (L24–89) and is wired into CI
(`.github/workflows/kask-invariants.yml`). Running it locally
closes a fast loop (seconds) instead of waiting for the CI round-trip.

1. `bash kask/scripts/build/check-zed-isolation.sh`
2. If it fails, it names the offending path — re-delete that path and re-run.
3. Repeat until it passes. Bound: max 3 sweep rounds; a fourth failure means
   the merge restored something structural — halt and report instead of
   re-deleting. (`check-desktop-no-collision.sh` is a one-line alias
   for the same script — running either is sufficient; do not run both.)

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
   upstream work.
1. `cargo check -p <crate>` — the file compiles.
2. `cargo test -p <crate> -- <pinning tests>` — all pinning tests pass.
3. `bash kask/scripts/check-hkask-no-zed-deps.sh` — §13.1 invariant holds.
4. `grep -c "// zed-kask:" <file>` — marker count matches the functional unit count.
5. `git diff upstream/main -- <file>` — no upstream line is modified except under a live D-row (retire/simplify work may remove fork lines).

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
| `assess.j2` | Assess a D-seam file against the strategy decision rule. Extract line counts, kask call site count, marker count. Recommend merge vs. mapped re-application. |
| `map.j2` | Extract the functional inventory (F1, F2, ...), classify each unit by constraint force, and build the dependency DAG. |
| `decide.j2` | Apply the essentialist deletion test: is full re-application necessary, or is surgical marking + pinning sufficient? |
| `execute.j2` | Execute the chosen strategy: add markers + pinning tests (surgical), or re-apply onto clean upstream in topological order (full re-application). |
| `document.j2` | Update DIVERGENCE.md and produce the final report. |
| `reflect.j2` | After the merge commit: compare what happened against this skill, and propose amendments each tied to a falsifiable held-out case. |

To render a template, call the `render_template` tool with the template ref (e.g., `upstream-rebase/assess`) and a context object with the required variables.

Run verification gates (cargo check/test, isolation script) with `terminal`; use `lisp_eval` only for arithmetic such as marker density. Template order: Step 0 survival over all D-rows (`decide.j2`), then `assess` → `map` → `execute` → `document` for surviving seam files only, then `reflect` after the merge commit.

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

### Conflict classes

The `DIVERGENCE.md` runbook names three classes (D-seam files, workspace `Cargo.toml`
arrays, and the additive `kask/` tree that never conflicts). The table below
adds the two modify/delete classes the runbook omits:

| Class | Resolution |
| --- | --- |
| **D-seam modify/modify** | Follow the decision rule (`SKILL.md` decision rule + Step 1 scope note). Git-merge if well-marked; mapped re-application if under-marked. |
| **Kask-additive no-conflict** | No action (`DIV` L8–9, L76–79, L99). |
| **Workspace `Cargo.toml` arrays** | Hand-merge: keep both sides' entries (`DIV` L10–11, L98). Never drop a kask member. |
| **Modify/delete — upstream restores** | Run Step 8 (`check-zed-isolation.sh`); re-delete every path it names; re-run until pass. |
| **Modify/delete — upstream deletes a file zed-kask modifies** | Default (Hypothesis-tier, no instance in `DIV`): if the kask wiring is still load-bearing, re-add the file as a new D-seam row (move under `kask/` if possible). If obsolete, accept the deletion and remove the `DIV` row. Either way, add/update the pinning test in the same commit. |

### Commit hygiene

Follow `.rules` "Pull request hygiene": imperative PR title, no conventional-commit
prefix, `Release Notes:` as the final section with a blank line after the heading.
For a pure upstream sync: `- N/A`. Merge commit message: `Merge upstream/main
<upstream-sha> into zed-kask`. Do not squash — upstream's commit log is the audit
trail for what changed.

### Branch & PR strategy

Not found in `DIVERGENCE.md` or `.rules` (the `DIVERGENCE.md` runbook does not name
a branch). **Proposal:** land on `main` via a PR from a short-lived
`upstream-sync-<YYYY-MM-DD>` branch, created fresh per sync. Do not maintain a
long-lived `upstream-sync` branch — it would accumulate conflicts against both
`main` and `upstream/main`. Flagged as a proposal; verify the project's actual
convention before adopting.

### Verification gate ordering (merge-level, before pushing the PR)

The skill's per-file gate (above) is for a single re-applied D-seam file. The
merge-level gate below runs over the whole tree. Order: cheap Prohibition-class
invariants first, then compile, then tests.

1. `bash kask/scripts/build/check-zed-isolation.sh` — Zed-isolation + desktop
   no-collision (one script; `check-desktop-no-collision.sh` is a one-line alias
   per its L7, so do not run both).
2. `bash kask/scripts/check-hkask-no-zed-deps.sh` — §13.1 invariant (`DIV`
   L100–101).
3. `./script/clippy` — `.rules` build guidelines: "Use `./script/clippy` instead
   of `cargo clippy`." Runs under `--deny warnings`.
4. `cargo check -p kask_bridge -p hkask-types -p hkask-mcp-server` — `DIV`
   runbook step 5 (intent is
   "verify the bridge + foundation still compile").
5. `cargo test -p <affected-crates> -- <pinning-tests>` — per-file pinning tests
   from Step 6.

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
- **Pushed to PR branch, before merge to `main`:** `git push --force-with-lease
  origin upstream-sync-<YYYY-MM-DD>` — `--force-with-lease` (not `--force`)
  rejects if someone else pushed.
- **Merged to `main`:** do not force-push `main`. Revert with
  `git revert -m 1 <merge-sha>` and open a follow-up PR.
