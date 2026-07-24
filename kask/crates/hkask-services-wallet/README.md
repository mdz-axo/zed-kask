# hkask-services-wallet — Wallet Service

Multi-chain wallet service providing crypto wallet operations, chain port selection, and gas calibration. Implements the `WalletService` port trait.

**Version:** v0.31.0 | **Crate:** `hkask-services-wallet`

## Exports

| Type | Purpose |
|------|---------|
| `WalletService` | Multi-chain wallet operations (Hedera, optional Hinkal) |

## Configuration

| Variable | Required | Description |
|----------|----------|-------------|
| `HEDERA_ACCOUNT_ID` | Yes | Hedera account ID |
| `HEDERA_PRIVATE_KEY` | Yes | Hedera private key (DER/HEX) |
| `HEDERA_NETWORK` | No | Network: `mainnet`, `testnet` (default), `previewnet` |
| `HINKAL_API_KEY` | No | Hinkal shielded pool API key (optional) |

## Dependencies

- `hkask-wallet` — Core wallet crate (Ed25519, multi-chain)
- `hkask-wallet-types` — Value types (Hbar, GasEstimate, etc.)
- `hkask-services-core` — `ServiceConfig`, `ServiceError`
- `hkask-regulation` — Regulation span emission for wallet operations
