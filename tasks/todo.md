# D28 Documentation & Settings Alignment — TODO

## Phase A — Foundation

- [x] T1: Audit and fix kask-settings.md
  - [x] Every env var emitted by `mcp_env()` is documented
  - [x] `transactions_dir` default says `mcp/portfolio/transactions/`
  - [x] No references to `pod.db`, `agents/registry`, `AGENT_SUBDIRS`, `swarm_ledger`, `docproc-cache`
- [x] T2: Scan all docs for stale D28 references
  - [x] Complete hit list with file:line for each stale reference
  - [x] No false positives (`.agents/skills/` repo-root is not stale)

**Checkpoint A:** Settings doc clean; hit list complete. ✅

## Phase B — Pruning

- [x] T3: Delete stale status/continuation docs
  - [x] Deleted: `findings.md`, `reflection.md`, `canonical-patterns.md`, `prompt-comparative-analysis.md`, `kask/docs/status/public-seam-inventory.json`
  - [x] Deleted docs removed from all indexes
  - [x] No surviving doc links to a deleted doc

**Checkpoint B:** Dead docs pruned; no broken links. ✅

## Phase C — Documentation updates

- [x] T4: Update architecture docs
  - [x] No arch doc references `pod.db`, `agents/registry`, `AGENT_SUBDIRS`
  - [x] `memory-system-specification.md` references `curator.db`
  - [x] `adr-embedded-yaml-registry.md` references `skills/registry/`
  - [x] `PRINCIPLES.md` references `{sanitized_name}.db` (not `pod.db`)
  - [x] `AGENT_SYSTEM_PROMPT.md` broken link to deleted doc fixed
- [x] T5: Update reference docs
  - [x] `corpus.md` references `mcp/corpus/cache/` (not `docproc-cache`)
  - [x] `kask-settings.md` updated with all D28 env vars and defaults
- [x] T6: Update explanation docs
  - [x] `cognition-and-replica.md` references `curator.db` (not `pod.db`)
- [x] T7: Update diataxis docs
  - [x] `hkask-regulation/reference.md` references `curator.db`
  - [x] `hkask-capability/reference.md` references `curator.db`
  - [x] No diataxis doc has stale D28 refs (grep verified)

**Checkpoint C:** All docs updated; no stale references. ✅

## Phase D — Diagrams & indexes

- [x] T8: Regenerate stale diagrams
  - [x] `standardized-artifact-storage.md` has 4-class mermaid ERD diagram
- [x] T9: Repair indexes and crosslinks
  - [x] `kask/docs/README.md` — removed reference to deleted `public-seam-inventory.json`
  - [x] `AGENT_SYSTEM_PROMPT.md` — removed broken link to deleted `prompt-comparative-analysis.md`
  - [x] No broken internal links (grep verified)

**Checkpoint D:** Diagrams regenerated; indexes repaired. ✅

## Phase E — Validation

- [x] T10: Final validation
  - [x] `grep` for stale refs returns 0 hits (excluding DIVERGENCE.md history)
  - [x] `./script/clippy -p hkask-types -p kask_bridge` passes
  - [x] `cargo test -p hkask-types -p kask_bridge` passes (278 tests)
  - [x] No broken doc links

**Checkpoint E (final):** All docs aligned; all tests pass. ✅

## Follow-up (not in scope)

- Full diataxis doc regeneration from scratch (40 docs) — the user requested
  this but the D28-specific stale refs are already fixed. A full regeneration
  would catch non-D28 staleness but is a large separate task.
- `kask/docs/plans/` docs (abw-swarm-intelligence, cybernetic-swarm-plan,
  evolving-test-harness) — no stale D28 refs; kept for forward-looking value.