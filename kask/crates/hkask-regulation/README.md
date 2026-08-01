# hkask-regulation — Regulation System

Homeostatic self-regulation engine for hKask. Regulation enforces Ashby's Law of Requisite Variety through variety sensing, algedonic alerts, energy budgets, OCAP governance, and sovereignty enforcement (Loop 6).

## Public Modules

| Module | Purpose |
|--------|---------|
| `runtime` | `RegulationLedger` — central Regulation state machine |
| `cybernetics_loop` | Loop 6 main sense→compute→act cycle |
| `energy` | Gas budgets (`hJoules`), `GasBudget`, `GasCost` |
| `energy_budget_management` | Budget registration, reservation, settlement |

| `algedonic` | Algedonic signal channel (positive/negative valence) |
| `types::loops` | `CurationInput`, `LoopAction`, `CuratorDirective` |
| `wallet_manager` | Wallet-backed energy budgets |
| `runtime_policy` | Layer 6 defense — pre-execution policy check (VeriGuard/AgentGuard) |

## Key Types

| Type | Description |
|------|-------------|
| `RegulationLedger` | Central Regulation state machine with health, variety, alerts |
| `CyberneticsLoop` | Loop 6 regulation cycle |

The OCAP-gated tool invocation membrane (`McpRuntime::invoke` / `ToolGovernance`) lives in `hkask-mcp`; it consumes this crate's `CyberneticsLoop`, `GasBudget`, and `ToolStats` primitives via the hold-settle pattern.

| `GasBudget` | Energy budget with hJoule accounting |
| `SetPoints` | Configurable regulatory thresholds |
| `DefaultPolicy` | Pre-execution policy check (Allow/Block/RequireHuman/Log) — FIDES taint flow + rate limiting + human-in-the-loop enforcement |

## Dependencies

- `hkask-types` — foundation types (WebID, NuEvent, InfrastructureError, InferencePort)
- `hkask-storage` — persistence
- `hkask-capability` — OCAP delegation tokens
- `tokio`, `tracing`, `serde`, `chrono`
