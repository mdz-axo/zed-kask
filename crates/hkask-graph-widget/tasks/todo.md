# Graph widget capability plan — todo

## Phase 1 — Correctness (Prohibition)

- [x] **T2. Warn on `>20 parents` silent fallback in `propagate.rs`** — code complete (compiles via `diagnostics`; runtime `cargo test` deferred to user — `terminal` tool rejected the command shape)
  - [x] A node with 21 parents and evidence on a parent emits `tracing::warn!` with `target: "hkask-graph-widget"`, naming the node id and "falling back to base marginal"
  - [x] Returned marginal is still the base value (behavior preserved, signal added)
  - [x] Test `high_fan_in_falls_back_to_base_marginal` asserts the fallback returns the base marginal (warn verified by code review — no tracing-subscriber dev-dep)
  - Verify: `cargo test -p hkask-graph-widget propagate` (deferred to user)

**Checkpoint 1:** `cargo test -p hkask-graph-widget` — all pass, warn fires on high-fan-in.

## Phase 2 — Honesty (Guardrails)

- [x] **T3. Validate conditional tables at the math boundary (S4 layer)**
  - [x] `validate_conditionals(body) -> Vec<ConditionalWarning>` in `block.rs` checks each `depends_on[i].conditionals.len() == 2^parent_event_ids.len()`
  - [x] `recompute_marginals` calls `validate_conditionals` and emits `tracing::warn!` per warning before propagating
  - [x] `layout.rs` delegates its inline check to `validate_conditionals` (single source)
  - Verify: `cargo test -p hkask-graph-widget block validate` (deferred to user)
  - Depends: T2

- [x] **T4. Make `depends_on` schema honest: implement multi-dep conjunctive conditions**
  - [x] A node with two `depends_on` entries (over parents {A} and {B}) computes its marginal as `marginalize(A) * marginalize(B)` (independence)
  - [x] A node with one `depends_on` entry behaves exactly as before (no regression)
  - [x] Tests: `multi_dep_combines_by_independence`, `single_dep_no_regression`
  - Verify: `cargo test -p hkask-graph-widget propagate multi_dep` (deferred to user)
  - Depends: T2

- [x] **T7. Recompute `joint_probability` after evidence**
  - [x] After `set_evidence`, header shows recomputed joint probability (product of marginals under independence), not the stale server value
  - [x] `revert_to_base` restores the original server joint probability
  - [x] Test `joint_probability_recomputes_after_evidence` asserts the recomputed value (0.495) and the revert
  - Verify: `cargo test -p hkask-graph-widget view joint_probability` (deferred to user)
  - Depends: T5b (uses the same marginal source as the display) — implemented early since T5b not yet done; will re-verify after T5b

- [x] **T8. De-duplicate `certainty_tier` across `hkask-forecast` and `hkask-mcp-scenarios`** — **DROPPED (PDCA iter 4):** `CertaintyTier::from_probability` already delegates to `hkask_forecast::certainty_tier`. No duplication exists.

- [x] **T9. Delete stale TODO in `propagate.rs`**
  - [x] Stale TODO removed; replacement comment describes the residual (evidence-override wrapper is widget-only by design)
  - Verify: `cargo test -p hkask-graph-widget` (no behavior change)

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
