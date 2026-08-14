# D28 Documentation & Settings Alignment — TODO

## Phase A — Foundation

- [x] T1: Audit and fix kask-settings.md
- [x] T2: Scan all docs for stale D28 references

**Checkpoint A:** ✅

## Phase B — Pruning

- [x] T3: Delete stale status/continuation docs
  - Deleted: `findings.md`, `reflection.md`, `canonical-patterns.md`, `prompt-comparative-analysis.md`, `kask/docs/status/public-seam-inventory.json`

**Checkpoint B:** ✅

## Phase C — Documentation updates

- [x] T4: Update architecture docs
- [x] T5: Update reference docs
- [x] T6: Update explanation docs
- [x] T7: Regenerate diataxis docs from scratch (40 docs across 11 crates)

**Checkpoint C:** ✅

## Phase D — Diagrams & indexes

- [x] T8: Regenerate stale diagrams (mermaid ERD in standardized-artifact-storage.md)
- [x] T9: Repair indexes and crosslinks (INDEX.md updated, broken links fixed)

**Checkpoint D:** ✅

## Phase E — Validation

- [x] T10: Final validation
  - Stale ref grep: 0 hits (excluding legitimate "not pod.db" / "former" historical context)
  - `./script/clippy`: clean
  - `cargo test`: 278 tests pass
  - No broken doc links

**Checkpoint E (final):** ✅ All docs aligned; all tests pass.