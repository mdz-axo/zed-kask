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

- [x] **T5a. Polytree detection + backward inference core (Pearl π-λ)**
  - [x] `is_polytree(body) -> bool` returns true for chain/tree/polytree, false for diamond (union-find on undirected edges)
  - [x] `recompute_posteriors(body, topo_order, evidence) -> Vec<f64>` implements fixpoint forward+backward sweeps (Pearl π-λ)
  - [x] `debug_assert!(is_polytree(body))` encodes the polytree precondition (catches test-time misuse)
  - [x] Tests: `is_polytree_true_for_chain/tree`, `is_polytree_false_for_diamond`, `backward_inference_updates_parent_on_leaf_evidence`, `backward_inference_updates_sibling_on_evidence` (the discriminating test that catches the sibling-stale bug)
  - Verify: `cargo test -p hkask-graph-widget propagate polytree` (deferred to user)
  - Depends: T2, T3

- [x] **T5b. Backward inference fallback + view integration**
  - [x] `repropagate` uses posteriors if polytree + evidence set; else forward marginals + `tracing::warn!` for multiply-connected
  - [x] `backward_inference_available: bool` field on `GraphWidget`, computed once in `new`
  - [x] Visible header notice: "backward inference unavailable for this graph shape — evidence propagates forward only" when `!polytree && !evidence.is_empty()`
  - [x] No regression in existing forward-only tests
  - Verify: `cargo test -p hkask-graph-widget view backward` (deferred to user)
  - Depends: T5a

- [x] **T6. Soft evidence mode (likelihood ratios)**
  - [x] `EvidenceKind = Hard(f64) | Soft(likelihood_ratio: f64)` in `block.rs` with `apply(prior) -> f64` method
  - [x] `evidence: HashMap<usize, EvidenceKind>` on `GraphWidget` and `WhatIfBranch`
  - [x] Hard evidence behaves exactly as before (no regression) — `set_evidence` delegates to `set_evidence_kind(Hard(value))`
  - [x] Soft evidence with `LR = 1.0` is a no-op; `LR = 3.0` on prior 0.5 yields posterior 0.75 (test `soft_evidence_applies_bayesian_update`)
  - [x] New "Observe (LR 3:1)" click button in render calls `set_soft_evidence(idx, 3.0, cx)`
  - [x] View test `soft_evidence_updates_marginal_without_clamping` verifies P(a)=0.75, P(b)=0.475 after soft evidence on a
  - Verify: `cargo test -p hkask-graph-widget propagate soft_evidence` (deferred to user)
  - Depends: T5b

**Checkpoint 3:** Full `cargo test` across `hkask-forecast`, `hkask-graph-widget`, `hkask-mcp-scenarios`. Manual smoke: 3-node chain, set evidence on leaf, observe root marginal update. Set soft evidence, observe Bayesian update. Compare two branches, observe recomputed joint probability.
