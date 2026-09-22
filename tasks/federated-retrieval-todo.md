# Phase 1 federated retrieval checklist

Goal: `7e65977c-610c-4ec7-a830-021b2fac8092`
Baseline: `6b5f19a9c8d13758167309ad04e587cf79b56817`

## Foundation

- [x] **T1 — Open sealed sources read-only**
  - [x] Encrypted read-only open performs no migration, inventory, lock-file, or WAL mutation.
  - [x] Queries and sqlite-vec search work; writes fail.
  - [x] Missing/read-integrity controls preserve source bytes and metadata.
- [x] **T2 — Bind source identity**
  - [x] Load identity from exact DB, run identity, representation manifest, and index name.
  - [x] Reject current-schema/digest/model/prefix/dimension/file mismatches visibly; no legacy shim.
  - [x] Keep all corpus identities and paths out of production constants.

## Core feature

- [x] **T3 — Retrieve external passages**
  - [x] Return passage text plus complete source and record provenance.
  - [x] Surface missing text and embedding incompatibility.
  - [x] Exclude method-signals h_mems by construction.
- [x] **T4 — Return fused search results**
  - [x] Expose `curator_federated_search` with deterministic source-rank interleaving.
  - [x] Preserve healthy results and source status when another source fails.
  - [x] Distinguish unconfigured, invalid, incompatible, unavailable, and no-match states.

## Checkpoint A

- [x] Run mixed Curator/corpus/degradation benchmark; reject one stale oracle and replace it with a current standalone-grounded case.
- [x] Reconcile pre/post corpus bytes, timestamps, row counts, and sidecars.
- [ ] Run affected full crate suites, scoped `./script/clippy`, and `cargo check -p zed`.
- [ ] Present results for operator review before Phase 2.
