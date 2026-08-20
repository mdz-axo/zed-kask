---
name: upstream-rebase
description: "Manage upstream Zed rebases for zed-kask. Decides per-D-seam-file strategy (git merge vs. mapped re-application vs. destroy-and-rebuild), executes it, pins every kask-wiring deviation with a test, and updates DIVERGENCE.md."
---

# Upstream Rebase

Manage upstream Zed rebases for zed-kask, preserving the fork's functional
kask-wiring changes without carrying forward accumulated cruft.

## When to Use

- When bringing zed-kask up to upstream Zed's `main` (`git fetch upstream && git merge upstream/main`).
- When a D-seam file has accumulated divergence (under-marked `// zed-kask:` markers, compile bugs from incremental evolution, > 2× upstream line count).
- When auditing a D-seam file's marker density and pinning test coverage after a merge.
- When you need to decide, per file, whether to git-merge or mapped-re-apply.

## The three strategies

| Strategy                  | When to use                                                                                   | Risk         | Effort |
| ------------------------- | --------------------------------------------------------------------------------------------- | ------------ | ------ |
| **Git merge**             | File is well-marked (every deviation has `// zed-kask:` + test) and compiles cleanly          | Low          | Low    |
| **Mapped re-application** | File is under-marked (< 50% of kask call sites carry markers), has cruft, or has compile bugs | Medium       | High   |
| **Destroy and rebuild**   | Never                                                                                         | Catastrophic | High   |

**Decision rule:** if the fork's file has > 2× the upstream line count, or < 50% of kask call sites carry `// zed-kask:` markers, use mapped re-application. Otherwise use git merge.

## The mapped re-application process (Steps 1–7) + post-rebase cleanup (Step 8)

**Scope:** Steps 1–7 apply to D-seam *files* — rows whose `DIVERGENCE.md` file
column names an existing file. Deletion D-seams (file column `—` or
~~struck through~~, e.g. D4, D10) are not files; skip Steps 1–7 for them and
proceed to Step 8 to verify the merge did not silently restore them.

### Step 1 — Establish the functional inventory (code-graph extraction)

Extract every kask-wiring functional unit from the fork's file. A _functional unit_ is a contiguous block implementing one kask capability. Use `graph-audit` code mode (via `hkask-mcp-codegraph` MCP server) when available; otherwise extract manually via `git diff upstream/main HEAD -- <file>` + `grep` for section headers and kask symbols.

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

### Step 6 — Pin every deviation with a test

Per the `.rules` trap "Tests must pin deliberate zed-kask deviations from upstream": every `// zed-kask:` marker must have a corresponding test. For process-global hooks (e.g., `main.rs` wirings), the pinning test is typically a compile-time + symbol-existence pin asserting the key types/functions are accessible.

### Step 7 — Update DIVERGENCE.md

Update the D-seam row to reflect the re-applied file: list the file, document every functional unit's constraint force and pinning test.

### Step 8 — Run post-rebase cleanup

`git merge upstream/main` can restore files zed-kask deliberately deleted under
D7/D16 (icons, `.desktop` templates, `script/bundle-mac`, Flatpak/Snap resources,
release workflows). `kask/scripts/build/check-zed-isolation.sh` is the enforcement
point — it enumerates every forbidden path (L24–89) and is wired into CI
(`.github/workflows/kask-ci.yml` invariants job, L196–197). Running it locally
closes a fast loop (seconds) instead of waiting for the CI round-trip.

1. `bash kask/scripts/build/check-zed-isolation.sh`
2. If it fails, it names the offending path — re-delete that path and re-run.
3. Repeat until it passes. (`check-desktop-no-collision.sh` is a one-line alias
   for the same script — running either is sufficient; do not run both.)

Do not re-list the forbidden paths here — the script is the authority and its
list updates independently of this skill.

## Verification gate (before committing)

1. `cargo check -p <crate>` — the file compiles.
2. `cargo test -p <crate> -- <pinning tests>` — all pinning tests pass.
3. `bash kask/scripts/check-hkask-no-zed-deps.sh` — §13.1 invariant holds.
4. `grep -c "// zed-kask:" <file>` — marker count matches the functional unit count.
5. `git diff upstream/main -- <file>` — the diff is _only_ kask additions (no upstream code modified outside the D-seam).

## Composed Skills

| Skill               | Role                                                                                          | When Invoked  |
| ------------------- | --------------------------------------------------------------------------------------------- | ------------- |
| `graph-audit`       | Code-graph extraction (code mode) + constraint-force classification (semantic mode)           | Steps 1–2     |
| `essentialist`      | Deletion test: is full re-application necessary, or is surgical marking + pinning sufficient? | Before Step 5 |
| `coding-guidelines` | Surgical re-application guardrails                                                            | Step 5        |
| `task-breakdown`    | Slice the re-application into vertical tasks                                                  | Step 5        |

## Ontological Anchors

- **PKO** (Procedural Knowledge Ontology): the re-application is a Procedure (specification → execution → verification). Each functional unit is a Step; the DAG is the StepExecution order; the pinning test is the StepVerification.
- **Pragmatic-semantics**: constraint-force classification (Prohibition/Guardrail/Guideline) determines which units are load-bearing.
- **Cybernetics**: the verification gate is a feedback loop (compile → test → invariant check → marker count).

## Process Document

The full process, with the `main.rs` functional inventory (28 units), DAG, and constraint-force classification, is in `kask/docs/upstream-rebase-process.md`.

## Registry Templates

| Template | Type | Purpose |
|----------|------|---------|
| `assess.j2` | KnowAct | Assess a D-seam file against the strategy decision rule. Extract line counts, kask call site count, marker count. Recommend merge vs. mapped re-application. |
| `map.j2` | KnowAct | Extract the functional inventory (F1, F2, ...), classify each unit by constraint force, and build the dependency DAG. |
| `decide.j2` | KnowAct | Apply the essentialist deletion test: is full re-application necessary, or is surgical marking + pinning sufficient? |
| `execute.j2` | KnowAct | Execute the chosen strategy: add markers + pinning tests (surgical), or re-apply onto clean upstream in topological order (full re-application). |
| `document.j2` | KnowAct | Update DIVERGENCE.md and produce the final report. |
| `document.j2` | KnowAct | Update DIVERGENCE.md and produce the final report. |

The cascade also runs deterministic compute steps between the LLM steps: a `lisp.eval` verification gate (cargo check/test, isolation script, marker density) after execute, a `shell.exec` collision-surface cleanup after document, and a `lisp.eval` convergence signal before the loop.

## Constraints

- Do NOT modify any upstream file outside the D-seam surface. Consult `DIVERGENCE.md`'s divergence-surface table for the current D-seam rows — the table is authoritative; do not rely on a hardcoded range label (the count drifts as seams are added). If an upstream edit seems necessary, propose a new D-seam entry in `DIVERGENCE.md`.
- Do NOT rename or reformat upstream files to "fix" them.
- Every `// zed-kask:` deviation preserved or introduced must have a corresponding test.
- The re-application order must be a topological sort of the dependency DAG (no use-before-def).
- Prefer surgical marking + pinning over full re-application when the fork's file already compiles and is correctly ordered (essentialist G1: identical end state, lower risk).
- rJoule cap: 5 per invocation. Maximum 10 iterations.

## Merge & rebase protocol

### Fetch & merge strategy

The project convention is **merge, not rebase** (`DIVERGENCE.md` runbook step 1,
L95: `git fetch upstream && git merge upstream/main`). A long-lived fork tracking
upstream `main` merges — rebasing would rewrite fork history and force-push,
breaking collaborator branches. Preserve upstream history; do not squash.

### Conflict classes

`DIVERGENCE.md` L96–98 names three classes (D-seam files, workspace `Cargo.toml`
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

Not found in `DIVERGENCE.md` or `.rules` (the runbook `DIV` L93–102 does not name
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
   of `cargo clippy`." Runs under `--deny warnings` (D22 exists because two pins
   failed this gate).
4. `cargo check -p kask_bridge -p hkask-types -p hkask-mcp-server` — `DIV`
   runbook step 5 (L102; the runbook sentence is truncated there — intent is
   "verify the bridge + foundation still compile").
5. `cargo test -p <affected-crates> -- <pinning-tests>` — per-file pinning tests
   from Step 6.

### Recovery

- **Mid-merge, uncommitted:** `git merge --abort` — returns to pre-merge HEAD.
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

`git stash` is not appropriate for mid-re-application recovery — mapped
re-application is a deliberate multi-file edit, not an interruptible stashable
change. Use `git reset --hard` or `git checkout --`.
