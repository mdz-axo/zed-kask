---
name: doc-update
description: Realign the kask/docs tree with the current code. Full recomposition per docs-set (ground → compare → recompose → verify) under the <70-document condensation cap, with role-based triage, file:line citation discipline, and the corpus-tool decision point. Use when code changes have drifted docs, when adding a new crate/server to the documented surface, or on a scheduled docs refresh.
---

# doc-update

Realign `kask/docs/` with the code it documents. Documentation drift is not
cosmetic: a stale doc is active misinformation — agents read comments and docs
as ground truth and will follow them over user instructions. This skill exists
to make every documented claim verifiable against the current tree.

## Ontological anchors

- **Diataxis** (Procida) — the quadrant taxonomy (Tutorial / How-to /
  Reference / Explanation) that organizes the per-crate doc sets. A doc's
  quadrant determines its recomposition standard.
- **Minimalism** (Carroll, *The Nurnberg Funnel*) — fewer, task-oriented
  documents beat comprehensive ones. Anchors the condensation cap: the tree
  holds fewer than 70 documents; leaf docs without a formal role are folded
  or deleted, never left to rot.
- **PKO** (Procedural Knowledge Ontology) — this skill IS a procedure with
  specification/execution separation: the SKILL.md is the spec, the phases
  below are the execution.
- **Evidence-grounded documentation** — every factual claim cites `file:line`
  in the current tree; advertised invariants point to their enforcement line
  or say "not yet enforced". A claim without a resolvable citation is
  fabricated until proven otherwise.

## When to Use

- Code changed and the docs describing it may have drifted (renames, deleted
  surfaces, new crates/servers, count changes).
- A new crate, MCP server, widget, or skill was added and needs documentation.
- A scheduled refresh, or the operator reports a doc contradicting the code.
- After an upstream rebase that moved D-seam files.

## When NOT to Use

- Writing a brand-new design doc or plan (that is authoring, not realignment).
- Fixing a single typo or link — do it directly; don't invoke the full loop.
- Code review of documentation-adjacent comments (use code-review).

## Governing specs (read before editing)

1. `kask/docs/architecture/DOCUMENTATION_STANDARDS.md` — frontmatter,
   Mermaid-First, DIAGRAM_ALIGNMENT, lifecycle, the <70 cap (§3), verification
   checklist (§10).
2. `kask/docs/architecture/core/MDS.md` — the surviving-crate list
   (Composition Root). A doc naming a crate not in that table, or a deleted
   surface, is stale by definition.
3. `kask/docs/README.md` — the portal and the **document lifecycle ledger**:
   every fold/delete must be recorded there with its successor.

## Instructions

### Phase 0 — Condensation triage (before any recomposition)

1. Count the tree: `find kask/docs -type f | wc -l`. The cap is **< 70**.
   If the realignment would add documents, the triage must first make room.
2. Classify every candidate artifact by role:
   - **CORE** — referenced from a governing spec (README, INDEX,
     DOCUMENTATION_STANDARDS, MDS) or mechanically consumed (CI hooks,
     registries). Keep.
   - **FOLD** — content worth keeping whose file lacks a formal role. Merge
     into its natural successor (spec absorbs explanation; reference absorbs
     how-to; consolidated diagram file absorbs per-diagram files). Delete the
     source after folding.
   - **DELETE** — stale plans, implemented designs (successor = the code or
     skill that implemented them), point-in-time audits, docs referencing
     deleted surfaces, docs duplicating diataxis/reference coverage.
3. Render the triage output shapes: call `render_template` with template ref
   `doc-update/triage` (the template lives in
   `kask/registry/templates/doc-update/triage.j2` — templates are registry
   resources, NOT files next to SKILL.md). Record every decision with a
   successor.
4. Never delete without a successor named. Git history is the content
   archive; the ledger is the map to it.

### Phase 1 — Ground (per docs-set, one pass each)

1. Identify the crate(s) the docs-set documents. Extract the actual public
   surface: modules, tools, types, constants, invariants. Use `grep` /
   `find_path` / `read_file`; read the lib root and key modules.
2. **Corpus-tool decision point.** State the choice per pass:
   - **Direct reading** (default) — crates under ~10K LOC or with a handful
     of modules. Grounding by grep + targeted reads is faster and the
     citations are exact.
   - **Corpus pipeline** (`corpus_convert` → `corpus_chunk` → `corpus_embed`
     → `corpus_tag_chunks` → `corpus_query`) — only when a pass exceeds what
     direct reading can ground reliably (very large crates, cross-cutting
     questions over many files). If used, still re-verify every citation
     against the raw file before writing it — the pipeline finds evidence,
     it does not certify line numbers.
3. Prefer authoritative counts over greps: a pinning test
   (`tool_surface_is_exactly_N_registered_tools`) or a build-script-generated
   list beats an attribute grep. When only a grep is available, state the
   method in the doc.

### Phase 2 — Compare

1. Read the existing docs. Classify every factual claim:
   - **current** — verified against code this pass.
   - **stale** — was true, no longer is (renames, moved lines, changed
     behavior, count drift).
   - **fabricated** — never true or unverifiable (phantom types, tests that
     don't exist, aspirational behavior written as current).
2. Hunt specifically for: deleted surfaces presented as current; counts
   (crates, servers, tools, skills) that drifted; `file:line` refs that no
   longer resolve; links to deleted docs; advertised invariants with no
   enforcement line.

### Phase 3 — Recompose

1. Rewrite the artifact so every factual claim cites `file:line` in the
   current tree. IS ≠ OUGHT: never document aspirational behavior as
   current; mark it "not yet enforced" or delete it.
2. Advertised invariants point to their enforcement line (the test, the
   gate, the check). If none exists, the doc says "not yet enforced".
3. Preserve the Diataxis quadrant purpose of each file and its YAML
   frontmatter; update `last_updated` to the edit date.
4. When behavior changed, every doc describing the old behavior is updated
   in the same pass — a stale doc is worse than no doc.

### Phase 4 — Diagrams

1. Every Mermaid diagram is verified against current structure before it
   ships: node names exist, counts match, flows match the code path.
2. Each diagram carries a `DIAGRAM_ALIGNMENT` metadata block (unique id,
   `verified_date`, `verified_against` citing code files, `status`).
3. If a diagram's subject was deleted, drop it and note the deletion in the
   registry (`kask/docs/DIAGRAMS_INDEX.md`).

### Phase 5 — Reconcile

1. Update `diataxis/INDEX.md` counts and links to match the tree.
2. Update the README portal rows and the lifecycle ledger.
3. Update registries that carry counts (MCP server registry, skills
   registry, diagram registry) — a count in a doc is a claim like any other.
4. Sweep for links to deleted artifacts and repoint them to successors.

### Phase 6 — Verify (the gates)

Run every gate; all must pass before the pass is done:

1. **Count gate**: `find kask/docs -type f | wc -l` → must be < 70.
2. **Link gate**: sweep every `](target)` relative link in every `.md`;
   zero may be unresolved.
3. **Citation gate**: sample at least 5 `file:line` citations per recomposed
   artifact and verify each resolves to the claimed content (`sed -n` /
   `grep`). If any fails, re-verify all citations in that artifact.
4. **Frontmatter gate**: every `.md` has the six-field metadata header with a
   valid `status`.
5. **No-deleted-surfaces gate**: grep the recomposed docs for names of
   deleted crates/files; only tombstone mentions ("no longer exists") are
   allowed.

Convergence check — call `lisp_eval` with the gate results:

```
form: "(and count_ok links_ok citations_ok frontmatter_ok no_deleted_ok)"
env:  { "count_ok": <true|false>, "links_ok": <true|false>,
        "citations_ok": <true|false>, "frontmatter_ok": <true|false>,
        "no_deleted_ok": <true|false> }
```

If any gate is false, re-enter the failing phase. Do not end the pass with a
known-failing gate; report the failure instead.

## Failure surfacing

If any tool call fails during a pass, call `curator_report_skill_use_issue`
with `skill_name: "doc-update"`, the failed tool, and the error — then
continue with the best available grounding. A grounding failure must degrade
the doc's claims (mark unverified), never silently pass them through.

## Constraints

- The cap is **fewer than 70 documents** in `kask/docs/` — enforced at
  Phase 0 and re-checked at Phase 6.
- Never document a deleted surface as current. Tombstones say "no longer
  exists" and name the commit or date.
- Never invent a tool name, test name, or count. Extract from the `#[tool]`
  attributes, pinning tests, or generated lists.
- Deletions require a named successor in the README lifecycle ledger.
- One agent process edits the tree at a time; a half-recomposed docs-set
  (broken links, unresolved citations) blocks other agents' validation.
- Prefer folding into an existing doc over creating a new file; prefer
  deleting a stale doc over patching it.