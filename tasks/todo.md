# D28 Documentation & Settings Alignment — TODO

## Phase A — Foundation

- [ ] T1: Audit and fix kask-settings.md
  - [ ] Every env var emitted by `mcp_env()` is documented
  - [ ] `transactions_dir` default says `mcp/portfolio/transactions/`
  - [ ] No references to `pod.db`, `agents/registry`, `AGENT_SUBDIRS`, `swarm_ledger`, `docproc-cache`
- [ ] T2: Scan all docs for stale D28 references
  - [ ] Complete hit list with file:line for each stale reference
  - [ ] No false positives (`.agents/skills/` repo-root is not stale)

**Checkpoint A:** Settings doc clean; hit list complete.

## Phase B — Pruning

- [ ] T3: Delete stale status/continuation docs
  - [ ] Each deleted doc passes essentialist deletion test
  - [ ] Deleted docs removed from all indexes
  - [ ] No surviving doc links to a deleted doc

**Checkpoint B:** Dead docs pruned; no broken links.

## Phase C — Documentation updates

- [ ] T4: Update architecture docs
  - [ ] No arch doc references `pod.db`, `agents/registry`, `AGENT_SUBDIRS`
  - [ ] `memory-system-specification.md` references `curator.db`
  - [ ] `adr-embedded-yaml-registry.md` references `skills/registry/`
- [ ] T5: Update reference docs
  - [ ] Each MCP server reference doc shows `mcp/{server_id}/` default path
  - [ ] `corpus.md` references `mcp/corpus/cache/` (not `docproc-cache`)
  - [ ] `swarm.md` references `mcp/swarm/ledger.db` and `mcp/swarm/consent.db`
- [ ] T6: Update explanation docs
  - [ ] `memory-system.md` references `curator.db`
  - [ ] `skills-and-composition.md` references `{kask_data_dir}/skills/`
  - [ ] `training-and-adapters.md` references `mcp/training/adapters/`
- [ ] T7: Update diataxis docs
  - [ ] No diataxis doc references `pod.db` or `agents/registry`
  - [ ] `INDEX.md` lists all surviving docs
  - [ ] File:line references point to current code

**Checkpoint C:** All docs updated; no stale references.

## Phase D — Diagrams & indexes

- [ ] T8: Regenerate stale diagrams
  - [ ] `standardized-artifact-storage.md` has 4-class mermaid diagram
  - [ ] `memory-system-specification.md` diagram shows `curator.db`
  - [ ] All mermaid diagrams render without syntax errors
- [ ] T9: Repair indexes and crosslinks
  - [ ] No broken internal links
  - [ ] `INDEX.md` and `SUMMARY.md` list all surviving docs
  - [ ] `DIVERGENCE.md` supporting-files section is current

**Checkpoint D:** Diagrams regenerated; indexes repaired.

## Phase E — Validation

- [ ] T10: Final validation
  - [ ] `grep` for stale refs returns 0 hits (excluding DIVERGENCE.md history)
  - [ ] `./script/clippy -p hkask-types -p kask_bridge` passes
  - [ ] `cargo test -p hkask-types -p kask_bridge` passes
  - [ ] No broken doc links

**Checkpoint E (final):** All docs aligned; all tests pass.