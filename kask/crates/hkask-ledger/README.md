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

## Transaction ownership (core-review T06, 2026-09-04)

`Ledger::commit` and `Ledger::debit_if_funds` reserve one SQLite pooled
connection and use an immediate RAII transaction for the entire operation.
Reference/idempotency checks and balance checks occur under that transaction's
write lock. Failed postings roll back the header and every earlier posting;
concurrent pool users cannot commit or roll back that operation's connection.
`debit_if_funds` returns the balance at its own commit, not a later writer's
balance. Transactional writes require the existing SQLite driver; unsupported
drivers return an explicit error rather than falling back to unbound statements.

This is a focused correction against the review and `Ledger`'s source
contracts. The older triple-entry description above and its missing linked spec
have not been ratified or reconstructed by this change.

## See Also

- [`docs/architecture/specs/hkask-ledger.md`](../../docs/architecture/specs/hkask-ledger.md) — Full specification
- [`docs/architecture/specs/rjoule-cost-system.md`](../../docs/architecture/specs/rjoule-cost-system.md) — rJoule cost system
- [`PRINCIPLES.md`](../../docs/architecture/core/PRINCIPLES.md) §P8 — Semantic Grounding
