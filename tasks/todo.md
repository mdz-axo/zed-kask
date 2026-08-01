# Minimalist Refactor — Todo

## Slice 1 — `EnergyEstimator` trait deletion test

- [ ] **S1-PDCA-1: Plan** — verdict target `remove`; deletion-test reasoning recorded in `tasks/plan.md`.
- [ ] **S1-PDCA-1: Do** — collapse `EnergyEstimator` trait + `Arc<dyn>` into concrete `FlatEnergyEstimator`:
  - [ ] Delete `kask/crates/hkask-regulation/src/energy_estimator.rs`
  - [ ] Remove `pub use energy_estimator::EnergyEstimator;` from `hkask-regulation/src/hkask_regulation.rs`
  - [ ] Remove `mod energy_estimator;` from `hkask-regulation/src/hkask_regulation.rs`
  - [ ] Move `FlatEnergyEstimator` struct + impls from `hkask-mcp/src/runtime.rs` (keep `estimate_cost` as inherent method, drop the trait impl)
  - [ ] Change `McpRuntime::with_governance` signature: `estimator: Arc<dyn EnergyEstimator>` → `estimator: FlatEnergyEstimator`
  - [ ] Change `ToolGovernance.estimator` field: `Arc<dyn EnergyEstimator>` → `FlatEnergyEstimator`
  - [ ] Update `crates/zed/src/main.rs:679` call site: pass `FlatEnergyEstimator::new()` directly (no `Arc::new`, no `dyn`)
  - [ ] Update `hkask-types/src/ports/regulation.rs:65` doc comment: remove stale `CalibratedEnergyEstimator` / `WalletGasCalibrator` references
  - [ ] Update `kask/crates/hkask-regulation/src/tool_stats.rs:10` doc comment: rephrase "the EnergyEstimator's point estimate" reference
- [ ] **S1-PDCA-1: Check** — run `cargo test -p hkask-regulation` (expect 92 pass) + `cargo test -p hkask-mcp` (expect 10 pass) + `cargo check -p zed` (composition root compiles)
- [ ] **S1-PDCA-1: Act** — if green, record verdict `remove` and close slice. If red, diagnose and iterate (max 9).
- [ ] **S1-Convergence:** tests green AND deletion-test verdict recorded AND no OUGHT claims remain.

## Final report

- [ ] Slices resolved / escape-hatched / blocked summary
- [ ] Before/after code graph (modules + edges)