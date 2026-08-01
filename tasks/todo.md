# Minimalist Refactor — Todo

## Slice 1 — `EnergyEstimator` trait deletion test

- [x] **S1-PDCA-1: Plan** — verdict target `remove`; deletion-test reasoning recorded in `tasks/plan.md`.
- [x] **S1-PDCA-1: Do** — collapsed `EnergyEstimator` trait + `Arc<dyn>` into concrete `FlatEnergyEstimator`:
  - [x] Deleted `kask/crates/hkask-regulation/src/energy_estimator.rs`
  - [x] Removed `pub use energy_estimator::EnergyEstimator;` from `hkask-regulation/src/hkask_regulation.rs`
  - [x] Removed `pub mod energy_estimator;` from `hkask-regulation/src/hkask_regulation.rs`
  - [x] Added inherent `estimate_cost` method to `FlatEnergyEstimator` in `hkask-mcp/src/runtime.rs` (dropped the trait impl)
  - [x] Changed `McpRuntime::with_governance` signature: `estimator: Arc<dyn EnergyEstimator>` → `estimator: FlatEnergyEstimator`
  - [x] Changed `ToolGovernance.estimator` field: `Arc<dyn EnergyEstimator>` → `FlatEnergyEstimator`
  - [x] Updated `crates/zed/src/main.rs:675` call site: pass `FlatEnergyEstimator::new()` directly (no `Arc::new`, no `dyn`)
  - [x] Updated `hkask-types/src/ports/regulation.rs:65` doc comment: replaced stale `GasReport, CalibratedEnergyEstimator, WalletGasCalibrator` with real `GasBudgetManager, WalletManager`
  - [x] Updated `kask/crates/hkask-regulation/src/tool_stats.rs:10` doc comment: rephrased to reference `FlatEnergyEstimator`'s flat point estimate
  - [x] Updated `kask/crates/hkask-regulation/src/wallet_manager.rs:35` doc comment: replaced stale `WalletGasCalibrator` with `WalletBudgetPort` trait reference
- [x] **S1-PDCA-1: Check** — `cargo test -p hkask-regulation` (92 pass) + `cargo test -p hkask-mcp` (10 pass) + `cargo test -p hkask-templates` (5 pass) + `cargo test -p kask_bridge` (92 pass) + `cargo check -p zed` (compiles) + `./script/clippy -p hkask-regulation -p hkask-mcp -p zed` (clean)
- [x] **S1-PDCA-1: Act** — green; verdict `remove` recorded; slice closed on iteration 1 of 9.
- [x] **S1-Convergence:** tests green AND deletion-test verdict recorded AND no OUGHT claims remain.

## Final report

- [x] Slices resolved / escape-hatched / blocked summary
- [x] Before/after code graph (modules + edges)