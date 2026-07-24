# hkask-ledger

Triple-entry accounting ledger for hKask. Tracks rJoule consumption, energy budgets, and cost settlement across the inference pipeline.

## Core Components

- `LedgerStore` — Persists debit/credit transactions
- `EnergyBudget` — Per-session energy allocation
- `CostTracker` — Real-time rJoule consumption tracking

## Architecture

The ledger implements triple-entry accounting:
1. **Debit entry** — Energy consumed (agent side)
2. **Credit entry** — Energy allocated (provider side)
3. **Audit entry** — Cryptographic proof linking debit and credit

All entries are immutable and timeline-ordered. The ledger is the single source of truth for energy accounting.

## See Also

- [`docs/architecture/specs/hkask-ledger.md`](../../docs/architecture/specs/hkask-ledger.md) — Full specification
- [`docs/architecture/specs/rjoule-cost-system.md`](../../docs/architecture/specs/rjoule-cost-system.md) — rJoule cost system
- [`PRINCIPLES.md`](../../docs/architecture/core/PRINCIPLES.md) §P8 — Semantic Grounding
