# Graph widget capability plan — todo

## Phase 1 — Correctness (Prohibition)

- [ ] **T2. Warn on `>20 parents` silent fallback in `propagate.rs`**
  - [ ] A node with 21 parents and evidence on a parent emits `tracing::warn!` with `target: "hkask-graph-widget"`, naming the node id and "falling back to base marginal"
  - [ ] Returned marginal is still the base value (behavior preserved, signal added)
  - [ ] Test asserts the warn fires (test subscriber)
  - Verify: `cargo test -p hkask-graph-widget propagate`

**Checkpoint 1:** `cargo test -p hkask-graph-widget` — all pass, warn fires on high-fan-in.

## Phase 2 — Honesty (Guardrails)

- [ ] **T3. Validate conditional tables at the math boundary (S4 layer)**
  - [ ] `validate_conditionals(body) -> Vec<ValidationWarning>` in `block.rs` checks each `depends_on[i].conditionals.len() == 2^parent_event_ids.len()`
  - [ ] `recompute_marginals` calls `validate_conditionals` and emits `tracing::warn!` per warning before propagating
  - [ ] `layout.rs` delegates its inline check to `validate_conditionals` (single source)
  - Verify: `cargo test -p hkask-graph-widget block validate`
  - Depends: T2

- [ ] **T4. Make `depends_on` schema honest: implement multi-dep conjunctive conditions**
  - [ ] A node with two `depends_on` entries (over parents {A} and {B}) computes its marginal as `marginalize(A) * marginalize(B)` (independence)
  - [ ] A node with one `depends_on` entry behaves exactly as before (no regression)
  - [ ] Test: 2-dep node with known conditionals produces the documented joint marginal
  - Verify: `cargo test -p hkask-graph-widget propagate multi_dep`
  - Depends: T2

- [ ] **T7. Recompute `joint_probability` after evidence**
  - [ ] After `set_evidence`, header shows recomputed joint probability (product of marginals under independence), not the stale server value
  - [ ] `revert_to_base` restores the original server joint probability
  - [ ] Test: set evidence on a 2-node chain, assert displayed joint probability changes
  - Verify: `cargo test -p hkask-graph-widget view joint_probability`
  - Depends: T5b (uses the same marginal source as the display)

- [ ] **T8. De-duplicate `certainty_tier` across `hkask-forecast` and `hkask-mcp-scenarios`** — **DROPPED (PDCA iter 4):** `CertaintyTier::from_probability` already delegates to `hkask_forecast::certainty_tier`. No duplication exists.

- [ ] **T9. Delete stale TODO in `propagate.rs`**
  - [ ] No stale TODO referencing the consolidation that already happened (formula delegates to `hkask_forecast::marginalize`)
  - [ ] If a comment remains, it accurately describes the residual (evidence-override wrapper is widget-only by design)
  - Verify: `cargo test -p hkask-graph-widget` (no behavior change)
  - Depends: None (independent)

**Checkpoint 2:** `cargo test -p hkask-forecast && cargo test -p hkask-graph-widget && cargo test -p hkask-mcp-scenarios` — all pass. Schema honest, validation fires, no duplication, no stale TODO.

## Phase 3 — Capabilities (Guidelines)

- [ ] **T5a. Polytree detection + backward inference core (Pearl π-λ)**
  - [ ] `is_polytree(body) -> bool` returns true for chain/tree/polytree, false for diamond
  - [ ] `recompute_posteriors(body, topo_order, evidence) -> Vec<f64>` implements Pearl π-λ message passing for singly-connected DAGs
  - [ ] Test: 3-node chain A→B→C, evidence on C, asserts A's marginal moves toward posterior
  - Verify: `cargo test -p hkask-graph-widget propagate polytree`
  - Depends: T2, T3

- [ ] **T5b. Backward inference fallback + view integration**
  - [ ] `repropagate` uses posteriors if polytree + evidence set; else forward marginals + `tracing::warn!` for multiply-connected
  - [ ] Setting evidence on a leaf in a diamond shows forward-only result + visible "backward inference unavailable for this graph shape" notice
  - [ ] No regression in existing forward-only tests
  - Verify: `cargo test -p hkask-graph-widget view backward`
  - Depends: T5a

- [ ] **T6. Soft evidence mode (likelihood ratios)**
  - [ ] `EvidenceKind = Hard(f64) | Soft(likelihood_ratio: f64)`; `evidence: HashMap<usize, EvidenceKind>`
  - [ ] Hard evidence behaves exactly as before (no regression)
  - [ ] Soft evidence with `LR = 1.0` is a no-op; `LR = 3.0` on prior 0.5 yields posterior 0.75
  - [ ] Test: soft evidence on a leaf propagates backward to parents (via T5) and forward to children
  - Verify: `cargo test -p hkask-graph-widget propagate soft_evidence`
  - Depends: T5b

**Checkpoint 3:** Full `cargo test` across `hkask-forecast`, `hkask-graph-widget`, `hkask-mcp-scenarios`. Manual smoke: 3-node chain, set evidence on leaf, observe root marginal update. Set soft evidence, observe Bayesian update. Compare two branches, observe recomputed joint probability.
