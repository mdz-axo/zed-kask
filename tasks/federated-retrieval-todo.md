# Phase 1 federated retrieval checklist

Goal: `7e65977c-610c-4ec7-a830-021b2fac8092`
Baseline: `6b5f19a9c8d13758167309ad04e587cf79b56817`

## Foundation

- [ ] **T1 — Open sealed sources read-only**
  - [ ] Encrypted read-only open performs no migration, inventory, lock-file, or WAL mutation.
  - [ ] Queries and sqlite-vec search work; writes fail.
  - [ ] Missing/wrong-key/read-integrity controls preserve source bytes and metadata.
- [ ] **T2 — Bind source identity**
  - [ ] Load identity from exact DB, run identity, representation manifest, and index name.
  - [ ] Reject digest/model/prefix/dimension/file mismatches visibly.
  - [ ] Keep all corpus identities and paths out of production constants.

## Core feature

- [ ] **T3 — Retrieve external passages**
  - [ ] Return passage text plus complete source and record provenance.
  - [ ] Surface missing text and embedding incompatibility.
  - [ ] Exclude method-signals h_mems by construction.
- [ ] **T4 — Return fused search results**
  - [ ] Expose `curator_federated_search` with deterministic source quotas and rank fusion.
  - [ ] Preserve healthy results and source status when another source fails.
  - [ ] Distinguish unconfigured, incompatible, unavailable, and no-match states.

## Checkpoint A

- [ ] Run mixed Curator/corpus/negative/degradation benchmark.
- [ ] Reconcile pre/post corpus bytes, timestamps, row counts, and sidecars.
- [ ] Run focused tests, affected crate suites, scoped `./script/clippy`, and `cargo check -p zed`.
- [ ] Present results for operator review before Phase 2.
