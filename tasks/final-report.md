# Minimalist Refactor — Final Report

## Executive summary

Seven speculative-generality traits removed from the `kask/` tree. All were
single-impl or zero-consumer traits that advertised polymorphism the code did
not exercise. Total: **-547 LOC source, -7 traits, -3 dead files, -1 dead
code path** (`WalletBackedBudget` → `wallet_budgets` map → sensor fallbacks).
Every test oracle green. No OUGHT claims remain.

## Slices resolved

| Slice | Target | Verdict | Iterations | Tests before → after |
|---|---|---|---|---|
| S1 | `EnergyEstimator` trait (hkask-regulation) | **remove** | 1/9 | 92→92, 10→10 |
| S2 | `EscalationPort` trait + mirror types (hkask-types) | **remove** | 1/9 | 72→72, 79→79 |
| S3 | `LedgerStoragePort` trait + `DecayConfig`/`WeightedEvent` (hkask-types) | **remove** | 1/9 | 72→72, 79→79 |
| S4 | `EmbeddingPort` trait + `StoredEmbedding` (hkask-types) | **remove** | 1/9 | 72→72, 79→79 |
| S5 | `WalletBudgetPort` trait + `WalletBackedBudget` dead path (hkask-types + hkask-regulation) | **remove** | 1/9 | 91→91 (1 test removed with its deleted production code) |
| S6 | `SkillReader` trait (hkask-templates) | **remove** | 1/9 | 130→130 |
| S7 | `RuntimePolicy` trait (hkask-regulation) | **remove** | 1/9 | 91→91, 130→130 |

## Slices escape-hatched

None.

## Slices blocked

None.

## What was removed

### S1: `EnergyEstimator` (8 LOC trait)
- **Why dead:** Single implementor (`FlatEnergyEstimator`), single call site
  (`McpRuntime::invoke`), held as `Arc<dyn>` for a polymorphism never exercised.
  The doc referenced hypothetical `CalibratedEnergyEstimator`/`WalletGasCalibrator`
  that never materialized.
- **What changed:** Collapsed to concrete `FlatEnergyEstimator` with inherent
  `estimate_cost` method. Removed `Arc<dyn>` indirection from `ToolGovernance`
  and `with_governance` signature.
- **Files:** Deleted `energy_estimator.rs`. Edited `runtime.rs`, `main.rs`,
  `hkask_regulation.rs`, `tool_stats.rs`, `wallet_manager.rs`, `regulation.rs`.

### S2: `EscalationPort` (100+ LOC trait + mirror types)
- **Why dead:** Zero `dyn` consumers. The curator MCP server uses the concrete
  `EscalationQueue` directly (`Arc<hkask_storage::EscalationQueue>`), never
  `Arc<dyn EscalationPort>`. The trait + its mirror types (`EscalationEntry`,
  `EscalationBatch`, `EscalationStatus`) + the `From` conversion impls existed
  only to serve a trait nobody held.
- **What changed:** Deleted `ports/escalation.rs` from hkask-types. Removed
  the `impl EscalationPort for EscalationQueue` block and `From` conversions
  from hkask-storage.
- **Files:** Deleted `ports/escalation.rs`. Edited `escalation.rs`,
  `ports/mod.rs`.

### S3: `LedgerStoragePort` (55 LOC trait + `DecayConfig`/`WeightedEvent`)
- **Why dead:** Zero `dyn` consumers. The curator MCP server uses the concrete
  `RegulationArchive` directly. The port-level `DecayConfig` and `WeightedEvent`
  were mirror types only used by the trait impl's `map_config` converter.
- **What changed:** Deleted the trait + mirror types from hkask-types. Removed
  the impl block + `map_config` helper from hkask-storage.
- **Files:** Edited `ports/regulation.rs`, `ports/mod.rs`, `regulation_store.rs`.

### S4: `EmbeddingPort` (25 LOC trait + `StoredEmbedding`)
- **Why dead:** Zero `dyn` consumers. All 5+ crates that use embedding storage
  (`hkask-mcp-corpus`, `hkask-mcp-training`, `hkask-mcp-condenser`,
  `hkask-mcp-curator`, `hkask-memory`) use the concrete `EmbeddingStore`
  directly. The actual embedding port used at runtime is
  `LanguageModelEmbeddingPort` in `kask_bridge` — a completely different type.
  The `EmbeddingPort` trait was confused dead code.
- **What changed:** Deleted `ports/embedding_port.rs`. Removed the impl block
  from hkask-storage.
- **Files:** Deleted `ports/embedding_port.rs`. Edited `embeddings.rs`,
  `ports/mod.rs`.

### S5: `WalletBudgetPort` + `WalletBackedBudget` (304 LOC dead path)
- **Why dead:** `register_wallet_budget` was **never called** from anywhere.
  The `wallet_budgets` map in `GasBudgetManager` was always empty. The entire
  `WalletBackedBudget` → `wallet_budgets` → `wallet_balance_ratios` →
  `wallet_key_alerts` → `WalletBalanceRatioSensor`/`WalletKeyHealthSensor`
  chain produced no data. The trait's doc claimed "hexagonal port" inversion,
  but the impl (`WalletManager`) and the consumer (`wallet_budget.rs`) were
  both in `hkask-regulation` — the same crate. No boundary was crossed.
- **What changed:** Deleted `wallet_budget.rs` (229 LOC) and
  `wallet_budget_port.rs` (75 LOC). Removed `wallet_budgets` field from
  `GasBudgetManager`, removed `register_wallet_budget` from
  `GasBudgetManager` and `CyberneticsLoop`, removed the `WalletBackedBudget`
  fallback paths from `can_proceed`/`reserve_gas`/`settle_gas`, replaced
  `wallet_balance_ratios`/`wallet_key_alerts`/`wallet_exhausted_agents` bodies
  with `Vec::new()` (preserving signatures for the sensor callers). Removed
  the `WalletBudgetPort` impl + `gas_per_rjoule` field from `WalletManager`.
- **Files:** Deleted `wallet_budget.rs`, `wallet_budget_port.rs`. Edited
  `energy_budget_management.rs`, `cybernetics_loop.rs`, `wallet_manager.rs`,
  `hkask_regulation.rs`, `ports/mod.rs`.
- **Test impact:** 1 test removed (`wallet_budget_gas_to_rjoules_conversion`)
  — it was inside the deleted `wallet_budget.rs`, testing the deleted
  `WalletBackedBudget::gas_to_rjoules` method. Correct per the task rule:
  "tests can be updated to reflect simplifications in the code."

### S6: `SkillReader` (5 LOC trait)
- **Why dead:** Single implementor (`FsSkillReader`), held as `Box<dyn>`.
  The doc claimed "tests wire a mock" but zero test impls existed. The trait
  was a purity seam for a mock that was never written.
- **What changed:** Moved `read_to_string` to an inherent method on
  `FsSkillReader`. Changed `SkillLoader.reader` from `Box<dyn SkillReader>`
  to `FsSkillReader`. Removed the misleading doc comment.
- **Files:** Edited `ports.rs`, `skill_loader.rs`, `hkask_templates.rs`.

### S7: `RuntimePolicy` (15 LOC trait)
- **Why dead:** Single implementor (`DefaultPolicy`), held as `Arc<dyn>` in
  `hkask-templates/src/executor.rs`. But `hkask-templates` already depends on
  `hkask-regulation` directly (via Cargo.toml) — the trait provided no
  dependency inversion. The "port" was in the same crate as the impl.
- **What changed:** Moved `check` to an inherent method on `DefaultPolicy`.
  Changed `runtime_policy` field and `with_runtime_policy` signature from
  `Arc<dyn RuntimePolicy>` to `Arc<DefaultPolicy>`.
- **Files:** Edited `runtime_policy.rs`, `hkask_regulation.rs`, `executor.rs`.

## Before/after code graph

### Before (7 speculative traits)

```
hkask-types/ports/
├── escalation.rs          [EscalationPort trait + EscalationEntry/Batch/Status mirror types]
├── embedding_port.rs      [EmbeddingPort trait + StoredEmbedding mirror type]
├── regulation.rs          [LedgerStoragePort trait + DecayConfig + WeightedEvent mirror types]
└── wallet_budget_port.rs  [WalletBudgetPort trait + WalletBudgetError]

hkask-regulation/src/
├── energy_estimator.rs    [EnergyEstimator trait]
├── wallet_budget.rs       [WalletBackedBudget struct (dead path, 229 LOC)]
├── wallet_manager.rs      [impl WalletBudgetPort for WalletManager (dead impl)]
└── runtime_policy.rs      [RuntimePolicy trait + impl for DefaultPolicy]

hkask-templates/src/
├── ports.rs               [SkillReader trait + impl for FsSkillReader]
└── executor.rs            [Arc<dyn RuntimePolicy> field]

hkask-mcp/src/runtime.rs   [Arc<dyn EnergyEstimator> field]

Edges (trait-mediated, all speculative):
  hkask-mcp → hkask-regulation::EnergyEstimator
  hkask-storage → hkask-types::EscalationPort (impl only, 0 consumers)
  hkask-storage → hkask-types::LedgerStoragePort (impl only, 0 consumers)
  hkask-storage → hkask-types::EmbeddingPort (impl only, 0 consumers)
  hkask-regulation → hkask-types::WalletBudgetPort (impl + consumer, same crate)
  hkask-templates → hkask-templates::SkillReader (impl + consumer, same crate)
  hkask-templates → hkask-regulation::RuntimePolicy (consumer already depends on impl crate)
```

### After (0 speculative traits)

```
hkask-types/ports/
├── (escalation.rs deleted)
├── (embedding_port.rs deleted)
├── regulation.rs          [LedgerObserver + ConsolidationRequest/Outcome + Depletion/Backpressure signals]
└── (wallet_budget_port.rs deleted)

hkask-regulation/src/
├── (energy_estimator.rs deleted)
├── (wallet_budget.rs deleted)
├── wallet_manager.rs      [WalletManager with inherent methods, no trait impl]
└── runtime_policy.rs      [DefaultPolicy with inherent check() method]

hkask-templates/src/
├── ports.rs               [FsSkillReader with inherent read_to_string()]
└── executor.rs            [Arc<DefaultPolicy> field]

hkask-mcp/src/runtime.rs   [FlatEnergyEstimator concrete field]

Edges (all concrete):
  zed → hkask-mcp::FlatEnergyEstimator (direct construction)
  hkask-templates → hkask-regulation::DefaultPolicy (direct, no dyn)
```

### Edge delta

- **Removed:** 7 trait-mediated edges (all speculative)
- **Removed:** 3 mirror-type conversion edges (`From` impls in hkask-storage)
- **Removed:** 1 dead code path (`WalletBackedBudget` → `wallet_budgets` → sensors)
- **Narrowed:** 4 `Arc<dyn>` / `Box<dyn>` indirections → concrete types

## Verification (all IS, no OUGHT)

| Crate | Tests before | Tests after | Status |
|---|---|---|---|
| hkask-types | 72 | 72 | green |
| hkask-storage | 79 | 79 | green |
| hkask-regulation | 92 | 91 | green (1 test removed with deleted `wallet_budget.rs`) |
| hkask-templates | 130 | 130 | green |
| hkask-mcp | 10 | 10 | green |
| kask_bridge | 92 | 92 | green |
| hkask-mcp-curator | 24 | 24 | green |
| hkask-mcp-corpus | 185 | 185 | green |
| hkask-memory | 56 | 56 | green |
| zed (compile) | — | — | compiles |
| clippy | — | — | clean |

## Cybernetic feedback-loop check

The test suite closed the loop (corrective, polarity = negative) on every
slice. No test went silent. The one test removal (`wallet_budget_gas_to_rjoules_conversion`)
was inside the deleted `wallet_budget.rs` file — it tested the deleted
`WalletBackedBudget::gas_to_rjoules` method. This is correct per the task
rule: "tests can be updated to reflect simplifications in the code."

## Suggested .rules additions

Per `.rules` "Rules Hygiene", I propose the following for reviewer
consideration:

> ## Zero-consumer port traits in hkask-types
>
> Port traits in `hkask-types/src/ports/` that have exactly one implementor
> (in `hkask-storage`) and zero `dyn` consumers are a recurring dead-code
> pattern. The trait + its mirror types + the `From` conversion impls in
> `hkask-storage` exist only to serve a trait nobody holds as `Arc<dyn>`.
> Before adding a new port trait to `hkask-types`, verify that at least one
> consumer will hold it as `Arc<dyn>` or `Box<dyn>` — not just that an
> implementor exists. Found in `EscalationPort`, `LedgerStoragePort`,
> `EmbeddingPort`, `WalletBudgetPort` (4 of 9 port traits were dead).
>
> ## `register_*` methods with zero call sites
>
> A `pub async fn register_*` method that has zero call sites outside its
> own definition is a dead entry point. The data structure it populates
> (e.g., `wallet_budgets: HashMap<...>`) is always empty, and every method
> that reads it returns empty/default. The dead path can extend far:
> `register_wallet_budget` → `wallet_budgets` map → `wallet_balance_ratios`
> → `WalletBalanceRatioSensor` → `SensorBus` registration. Before adding a
> `register_*` method, grep for call sites — if zero, the entire downstream
> chain is dead. Found in `GasBudgetManager::register_wallet_budget` (229 LOC
> of dead path).

These meet the three criteria: (1) non-obvious — the traits looked
legitimate from their definitions; (2) repeatedly encountered — 7 instances
across 4 crates; (3) specific enough to act on — "grep for `dyn` consumers
before adding a port trait" and "grep for `register_*` call sites."