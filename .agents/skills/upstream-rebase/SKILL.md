---
name: upstream-rebase
visibility: public
description: "Manage upstream Zed rebases for zed-kask. Decides per-D-seam-file strategy (git merge vs. mapped re-application vs. destroy-and-rebuild), executes the chosen strategy, pins every kask-wiring deviation with a test, and updates DIVERGENCE.md. For divergent files (under-marked, accumulated cruft, compile bugs), uses mapped re-application: extract the functional inventory (code-graph), classify each unit by constraint force (semantic audit), build the dependency DAG, re-apply onto clean upstream in topological order, and pin every unit with a test. Composes graph-audit (dual mode), essentialist, coding-guidelines, task-breakdown. Emits reg.upstream_rebase.* spans."
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

## The mapped re-application process (7 steps)

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

## Constraints

- Do NOT modify any upstream file outside the D1–D23 seams. If an upstream edit seems necessary, propose a new D-seam entry in `DIVERGENCE.md`.
- Do NOT rename or reformat upstream files to "fix" them.
- Every `// zed-kask:` deviation preserved or introduced must have a corresponding test.
- The re-application order must be a topological sort of the dependency DAG (no use-before-def).
- Prefer surgical marking + pinning over full re-application when the fork's file already compiles and is correctly ordered (essentialist G1: identical end state, lower risk).
