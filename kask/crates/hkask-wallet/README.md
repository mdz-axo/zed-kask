# hkask-wallet

Specialized wallet for hKask rJoule accounting, deposits, withdrawals, and API key capability issuance. The wallet is Hedera-only today, with chain ports wired through `ChainPort`.

## Core Components

- `WalletManager` — balances, deposits, withdrawal pipeline, deposit references
- `ChainPort` — chain adapter interface (currently Hedera)
- `signing.rs` — isolated treasury-key signing boundary
- `ApiKeyIssuer` — issues API key capabilities and stores metadata
- `WalletStore` (via `hkask-storage`) — SQLite-backed ledger and wallet state

## Supported Chains

| Chain | Status | Adapter |
|-------|--------|---------|
| Hedera | ✅ Core | `hedera` feature |

## Self-healing configuration

| Variable | Description |
|----------|-------------|
| `HKASK_DEPOSIT_REPAIR_MAX_INDEX` | Max derivation index scanned during deposit address repair (default 5, max 100) |

## See Also

- [`hkask-wallet-types`](../hkask-wallet-types/README.md) — Wallet value types
- [`hkask-keystore`](../hkask-keystore/README.md) — OS keychain integration
- [`PRINCIPLES.md`](../../docs/architecture/core/PRINCIPLES.md) §P1 — User Sovereignty
