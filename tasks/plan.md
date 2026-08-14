# Plan — D28 Documentation & Settings Alignment

> **Creator:** zed-kask agent
> **Date:** 2026-08-13
> **Status:** Active
> **DC/BIBO:** `bibo:Document`, `dct:title "D28 Documentation & Settings Alignment"`

## Overview

The D28 Standardized Artifact Storage work changed paths, env vars, and
module structure across the codebase. The code is updated and tested, but
the documentation and settings reference docs are stale. This plan aligns
all docs and settings with the current code, prunes dead docs, and
regenerates diagrams per the documentation standards.

## Architecture decisions

- **Settings audit first** — stale settings docs cause operator confusion;
  fix them before touching architecture docs.
- **Skill-informed doc review** — apply 6 skills as *lenses* (read-only
  evaluation), not as full cascades. Each skill flags docs that fail its
  criteria; the fixes are applied in one pass per doc.
- **Delete before rewrite** — stale docs that describe removed features
  are deleted first, so rewrites don't waste effort on dead content.
- **Mermaid-first** — per DOCUMENTATION_STANDARDS.md, diagrams live inline.
  Use the diataxis-diagram skill to generate diagrams from code where
  structure changed.

## Dependency graph

```
T1 (settings audit)     depth 0, no deps
T2 (stale doc scan)     depth 0, no deps
T3 (doc deletion)       depth 1, depends on T2
T4 (arch doc updates)   depth 1, depends on T3
T5 (reference doc updates) depth 1, depends on T3
T6 (explanation doc updates) depth 1, depends on T3
T7 (diataxis doc updates) depth 1, depends on T3
T8 (diagram regeneration) depth 2, depends on T4
T9 (index/crosslink repair) depth 2, depends on T4, T5, T6, T7
T10 (final validation)  depth 3, depends on T1, T9
```

## Phased task list

### Phase A — Foundation (settings + scan)

#### T1: Audit and fix kask-settings.md
- **slice_id:** `settings/audit-kask-settings`
- **Description:** Update `kask/docs/reference/kask-settings.md` to
  reflect all D28 changes: `transactions_dir` default is now
  `mcp/portfolio/transactions/` (not `<kask_data_dir>/transactions/`),
  add `HKASK_TRANSACTIONS_DIR` to the env-var table as emitted by
  `mcp_env()`, update the MCP server DB path defaults table to show
  `mcp/{server_id}/` paths, update the skills dir reference to
  `{kask_data_dir}/skills/`, update the threads DB reference to
  `{kask_data_dir}/threads/threads.db`, update `HKASK_CURATOR_DB` to
  reference `curator.db` (not `pod.db`), add `HKASK_SWARM_LEDGER_PATH`
  and `HKASK_SWARM_CONSENT_STORE` defaults.
- **Acceptance criteria:**
  - Every env var emitted by `mcp_env()` is documented in the env-var table.
  - `transactions_dir` default says `mcp/portfolio/transactions/`.
  - No references to `pod.db`, `agents/registry`, `AGENT_SUBDIRS`,
    `agents/curator/kanban.db`, `agents/curator/training.db`,
    `agents/curator/adapters`, `swarm_ledger.db`, `swarm_consent.db`,
    or `docproc-cache`.
- **Verification:** `grep -rn 'pod\.db\|agents/registry\|AGENT_SUBDIRS\|swarm_ledger\|docproc-cache' kask/docs/reference/kask-settings.md` returns 0 hits.
- **Dependencies:** None
- **Files likely touched:** `kask/docs/reference/kask-settings.md`
- **Estimated scope:** S

#### T2: Scan all docs for stale D28 references
- **slice_id:** `docs/stale-ref-scan`
- **Description:** Grep all `kask/docs/` and root-level `*.md` files for
  stale D28 references: `pod.db`, `agents/registry`, `AGENT_SUBDIRS`,
  `agents/curator/kanban`, `agents/curator/training`, `agents/curator/adapters`,
  `swarm_ledger`, `swarm_consent`, `docproc-cache`, `agents/skills/` (as a
  data path, not the repo-root `.agents/skills/` source tree), `data_dir()/agents/registry/`.
  Produce a hit list with file:line for each stale reference.
- **Acceptance criteria:**
  - A complete hit list is produced, categorized by doc.
  - No false positives (`.agents/skills/` repo-root source tree refs are
    not stale — they're the dev source tree, not the data dir).
- **Verification:** The hit list covers all `kask/docs/` + root `*.md`.
- **Dependencies:** None
- **Files likely touched:** None (read-only scan)
- **Estimated scope:** XS

**Checkpoint A:** Settings doc is clean; stale-ref hit list is complete.

### Phase B — Pruning

#### T3: Delete stale status/continuation docs
- **slice_id:** `docs/prune-stale`
- **Description:** Apply the essentialist deletion test to root-level
  status/continuation docs (`findings.md`, `reflection.md`,
  `canonical-patterns.md`, `prompt-comparative-analysis.md`) and
  `kask/docs/status/`, `kask/docs/plans/`. Delete docs that describe
  completed work with no forward-looking value. Consolidate overlapping
  docs. Remove deleted docs from all indexes.
- **Acceptance criteria:**
  - Each deleted doc passes the essentialist deletion test (deleting it
    loses no information not available in the current codebase or
    surviving docs).
  - Deleted docs are removed from `kask/docs/diataxis/INDEX.md`,
    `docs/src/SUMMARY.md`, and any cross-references.
  - No surviving doc links to a deleted doc.
- **Verification:** `grep -rn 'findings\.md\|reflection\.md\|canonical-patterns\|prompt-comparative' kask/docs/ docs/` returns 0 broken links.
- **Dependencies:** T2
- **Files likely touched:** Root `*.md`, `kask/docs/status/`, `kask/docs/plans/`, `kask/docs/diataxis/INDEX.md`, `docs/src/SUMMARY.md`
- **Estimated scope:** S

**Checkpoint B:** Dead docs are pruned; no broken links.

### Phase C — Documentation updates

#### T4: Update architecture docs
- **slice_id:** `docs/update-architecture`
- **Description:** Update `kask/docs/architecture/` docs to reflect D28:
  `zed-host-architecture-plan.md` (composition root references),
  `memory-system-specification.md` (curator.db, not pod.db),
  `adr-embedded-yaml-registry.md` (registry at skills/registry/, not
  agents/registry/), `AGENT_SYSTEM_PROMPT.md` (if it references storage
  paths). Apply deep-module lens: flag docs that describe dead surface.
  Apply pragmatic-semantics lens: flag OUGHT claims about removed features.
- **Acceptance criteria:**
  - No architecture doc references `pod.db`, `agents/registry`, or
    `AGENT_SUBDIRS`.
  - `memory-system-specification.md` references `curator.db`.
  - `adr-embedded-yaml-registry.md` references `skills/registry/`.
  - `zed-host-architecture-plan.md` composition root section references
    `mcp/{server_id}/` paths and `curator.db`.
- **Verification:** `grep -rn 'pod\.db\|agents/registry\|AGENT_SUBDIRS' kask/docs/architecture/` returns 0 hits.
- **Dependencies:** T3
- **Files likely touched:** `kask/docs/architecture/*.md`
- **Estimated scope:** M

#### T5: Update reference docs
- **slice_id:** `docs/update-reference`
- **Description:** Update `kask/docs/reference/` docs: `mcp-servers/`
  per-server docs (update DB path defaults to `mcp/{server_id}/`),
  `kask-settings.md` (already done in T1, verify), `regulation-spans.md`
  (if it references curator.db path), `ontology-bridge.md` (if it
  references storage paths). Apply grill-me Recall test: can a reader
  answer "what does this server's DB path default to?" from the doc alone?
- **Acceptance criteria:**
  - Each MCP server reference doc shows its `mcp/{server_id}/` default path.
  - `corpus.md` references `mcp/corpus/cache/` (not `docproc-cache`).
  - `swarm.md` references `mcp/swarm/ledger.db` and `mcp/swarm/consent.db`.
  - No reference doc mentions `pod.db` or `agents/registry`.
- **Verification:** `grep -rn 'pod\.db\|docproc-cache\|swarm_ledger\|swarm_consent' kask/docs/reference/` returns 0 hits.
- **Dependencies:** T3
- **Files likely touched:** `kask/docs/reference/mcp-servers/*.md`, `kask/docs/reference/*.md`
- **Estimated scope:** M

#### T6: Update explanation docs
- **slice_id:** `docs/update-explanation`
- **Description:** Update `kask/docs/explanation/` docs:
  `memory-system.md` (curator.db, not pod.db), `skills-and-composition.md`
  (skills dir is `{kask_data_dir}/skills/`, not `data_dir()/agents/skills/`),
  `cognition-and-replica.md` (replica artifacts are corpus-server-scoped,
  not agent-scoped), `training-and-adapters.md` (adapters at
  `mcp/training/adapters/`, not `agents/curator/adapters/`). Apply
  metacognition lens: verify feedback-loop docs reflect current wiring.
- **Acceptance criteria:**
  - `memory-system.md` references `curator.db` and `agents/curator/`.
  - `skills-and-composition.md` references `{kask_data_dir}/skills/` for
    the runtime skills dir (not `data_dir()/agents/skills/`).
  - `training-and-adapters.md` references `mcp/training/adapters/`.
  - No explanation doc mentions `pod.db` or `agents/registry`.
- **Verification:** `grep -rn 'pod\.db\|agents/registry' kask/docs/explanation/` returns 0 hits.
- **Dependencies:** T3
- **Files likely touched:** `kask/docs/explanation/*.md`
- **Estimated scope:** M

#### T7: Update diataxis docs
- **slice_id:** `docs/update-diataxis`
- **Description:** Update `kask/docs/diataxis/` per-crate docs to reflect
  D28 path changes. The diataxis docs cite concrete file:line references —
  verify these are still valid after the D28 changes. Update the INDEX.md
  if any doc was added or removed.
- **Acceptance criteria:**
  - No diataxis doc references `pod.db` or `agents/registry`.
  - `INDEX.md` lists all surviving docs (no deleted docs, no missing docs).
  - File:line references in diataxis docs point to current code.
- **Verification:** `grep -rn 'pod\.db\|agents/registry' kask/docs/diataxis/` returns 0 hits.
- **Dependencies:** T3
- **Files likely touched:** `kask/docs/diataxis/*.md`, `kask/docs/diataxis/INDEX.md`
- **Estimated scope:** M

**Checkpoint C:** All docs updated; no stale references remain.

### Phase D — Diagrams & indexes

#### T8: Regenerate stale diagrams
- **slice_id:** `docs/regenerate-diagrams`
- **Description:** Use the diataxis-diagram skill to regenerate mermaid
  diagrams in docs where the storage structure changed. Key diagrams:
  the storage layout diagram in `standardized-artifact-storage.md`
  (4 classes, not 5), the memory system diagram in
  `memory-system-specification.md` (curator.db), the composition root
  diagram in `zed-host-architecture-plan.md` (mcp/ paths). Per
  DOCUMENTATION_STANDARDS.md, diagrams live inline.
- **Acceptance criteria:**
  - `standardized-artifact-storage.md` has a mermaid diagram showing the
    4-class layout with example paths.
  - `memory-system-specification.md` diagram shows `curator.db` (not
    `pod.db`).
  - All mermaid diagrams render without syntax errors.
- **Verification:** Visual inspection of rendered mermaid in Zed.
- **Dependencies:** T4
- **Files likely touched:** `kask/docs/architecture/standardized-artifact-storage.md`, `kask/docs/architecture/memory-system-specification.md`, `kask/docs/architecture/zed-host-architecture-plan.md`
- **Estimated scope:** S

#### T9: Repair indexes and crosslinks
- **slice_id:** `docs/repair-indexes`
- **Description:** Update all indexes and cross-references after doc
  changes: `kask/docs/diataxis/INDEX.md`, `docs/src/SUMMARY.md`,
  `kask/docs/README.md`, `DIVERGENCE.md` supporting-files section,
  `README.md` root. Verify no broken links with `lychee`.
- **Acceptance criteria:**
  - No broken internal links.
  - `INDEX.md` and `SUMMARY.md` list all surviving docs.
  - `DIVERGENCE.md` supporting-files section references current file paths.
- **Verification:** `lychee` (or `grep` for common link patterns) reports 0 broken links.
- **Dependencies:** T4, T5, T6, T7
- **Files likely touched:** `kask/docs/diataxis/INDEX.md`, `docs/src/SUMMARY.md`, `kask/docs/README.md`, `DIVERGENCE.md`, `README.md`
- **Estimated scope:** S

**Checkpoint D:** Diagrams regenerated; indexes repaired; no broken links.

### Phase E — Validation

#### T10: Final validation
- **slice_id:** `validation/final`
- **Description:** Run the full validation sweep: grep for stale
  references across all docs, verify `./script/clippy` passes, verify
  `cargo test -p hkask-types` passes, verify `cargo test -p kask_bridge`
  passes, verify `lychee` link checker passes.
- **Acceptance criteria:**
  - `grep -rn 'pod\.db\|agents/registry\|AGENT_SUBDIRS\|swarm_ledger\|docproc-cache' kask/docs/ docs/ *.md` returns 0 hits (excluding DIVERGENCE.md historical references).
  - `./script/clippy -p hkask-types -p kask_bridge` passes.
  - `cargo test -p hkask-types -p kask_bridge` passes.
  - No broken doc links.
- **Verification:** All commands pass.
- **Dependencies:** T1, T9
- **Files likely touched:** None (validation only)
- **Estimated scope:** XS

**Checkpoint E (final):** All docs aligned; all tests pass; no stale references.

## Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Deleting a doc that's still referenced | Broken links | T9 repairs all indexes after deletions |
| Diataxis file:line refs are stale | Misleading docs | T7 verifies refs point to current code |
| Settings doc misses a new env var | Operator confusion | T1 cross-checks against `mcp_env()` source |
| Doc rewrite introduces inaccuracy | Worse than stale doc | T10 final grep sweep catches stale refs |

## Open questions

1. Should `findings.md` and `reflection.md` be deleted entirely or
   archived to a `kask/docs/archive/` dir? *Inference: delete — they
   describe completed work with no forward-looking value.*
2. Does `canonical-patterns.md` have forward-looking value (patterns
   still in use) or is it stale (patterns removed)? *Needs grep during T3.*
3. Should the diataxis per-crate docs be regenerated from scratch or
   patched? *Inference: patch — regeneration is expensive and most content
   is still valid.*
4. Are there any docs in `docs/src/` (the upstream Zed docs) that need
   kask-specific updates? *Inference: no — `docs/src/` is upstream Zed
   docs, not kask docs.*

## Refinement history

No PDCA iterations needed — the plan was stable on first decomposition.
The task count (10) is in the healthy range (3–20). All tasks are S or M
sized. No "and" in any task title. Every task has acceptance criteria,
verification, and declared dependencies.