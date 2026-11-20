---
title: "Wallet MCP Server (`hkask-mcp-wallet`) — Reference"
audience: [developers, architects, operators, agents]
last_updated: 2026-11-19
version: "0.42.0"
status: "Active"
domain: "Wallet / Payments"
mds_categories: [domain, composition, trust, security]
---

# Wallet MCP Server (`hkask-mcp-wallet`)

**Diataxis type:** Reference
**Divergence seam:** `D110`
**Design authority:** `kask/docs/research/wallet-cashu-lightning-design.md`

`hkask-mcp-wallet` is a Tier-0 financial MCP server providing Cashu e-cash (CDK `=0.18.3`, NUT-04/05/07/08/09/11/12/13/19) and Lightning (NIP-47 NWC executed editor-side behind `LightningRail`).

## Trust Boundaries & Custody

- **Zero launch credentials (`credentials: &[]`):** No secret enters `hkask-mcp-wallet`'s launch environment.
- **OS Keychain slots (editor-only, `kask_bridge::credentials`):**
  - `kask://credentials/wallet_seed_mnemonic` — BIP-39 12/24-word mnemonic (never leaves the editor; editor derives the 64-byte NUT-13 seed on `WalletDbUnlock`).
  - `kask://credentials/wallet_db_passphrase` — 32-byte hex SQLCipher key delivered once over authenticated `HKASK_INFERENCE_SOCKET` (`host/wallet_db_unlock`).
  - `kask://credentials/wallet_nwc_uri` — NIP-47 `nostr+walletconnect://` URI read exclusively by `kask_bridge::nwc_rail` (`hkask-mcp-wallet` does not link `nwc`).
  - `kask://credentials/wallet_mainnet_consent` — Keychain-backed mainnet consent belt (T10) written by Settings → Kask → Wallet.
- **Mandatory native GPUI spend approval:** Every spend (`wallet_pay_invoice`, `wallet_send_ecash`, `wallet_restore_from_seed`) crosses `HKASK_INFERENCE_SOCKET` into `kask_bridge::wallet_approval` (`host/wallet_spend_approve`) for a native modal showing rail, network, exact sats, max fee, destination, and remaining 24h cap. **No `autopublish` bypass exists.** No active window = refusal with zero keychain reads.
- **D100 Private-Thread Perimeter:** `"wallet"` is in `DEFAULT_PRIVATE_THREAD_DISABLED_SERVERS`.

## Storage (D28)

`<data_dir>/mcp/wallet/wallet.db` — SQLCipher-encrypted SQLite (`chmod 0600` on Unix, WAL mode, `synchronous = FULL`) holding:
- CDK wallet tables (`cdk-sql-common` adapted onto workspace `rusqlite@0.32`).
- Append-only `wallet_ledger` audit table with `UNIQUE(idempotency_key)` and partial unique active outbound `payment_hash` index (T8).

## Tool Surface (10 Tools)

Pinned by `tool_surface_pin!` against `hkask_types::wallet_wire::WALLET_TOOL_NAMES` with dual-axis P5.4 ontology annotations (`src/ontology.rs`):

| Tool | Class | PKO / DC-BIBO | Purpose |
|------|-------|---------------|---------|
| `wallet_status` | Read-only | `pko:ProcedureExecution` / `bibo:Report` | Network, `mainnet_enabled`, caps, allowlisted mints, NWC label, DB unlock state, pending reconciliation count |
| `wallet_balance` | Read-only | `pko:ProcedureExecution` / `bibo:Report` | Per-mint Cashu balances (confirmed/pending/reserved) and editor-queried NWC balance |
| `wallet_transactions` | Read-only | `pko:ProcedureExecution` / `dcterms:Dataset` | Paginated `wallet_ledger` query (never emits raw tokens or secrets) |
| `wallet_decode_payment` | Read-only (local) | `pko:ProcedureExecution` / `bibo:Report` | Pure-local decode of BOLT11, Lightning Address, or Cashu `cashuA`/`cashuB` token |
| `wallet_reconcile` | Repair (non-spending) | `pko:StepExecution` / `bibo:Report` | CDK incomplete saga recovery, NUT-07 pending proof check, paid NUT-04 mint issuance, and NWC `lookup_invoice` reconciliation |
| `wallet_create_invoice` | Receive (non-spending) | `pko:StepExecution` / `dcterms:Dataset` | Create BOLT11 receive invoice via Cashu NUT-04 mint quote or editor-side NWC `make_invoice` |
| `wallet_receive_ecash` | Receive (non-spending) | `pko:StepExecution` / `dcterms:Dataset` | Allowlist check, NUT-12 DLEQ verification, NUT-11 P2PK redemption, and proof swap |
| `wallet_pay_invoice` | **Spend (approval-gated)** | `pko:StepExecution` / `dcterms:Dataset` | BOLT11 validation (T6), idempotency (T8), native editor approval, Cashu NUT-05 melt (+ NUT-08 change) or editor-side NWC pay with SHA-256 preimage verification |
| `wallet_send_ecash` | **Spend (approval-gated)** | `pko:StepExecution` / `dcterms:Dataset` | Idempotency (T8), native editor approval, bearer or NUT-11 P2PK-locked Cashu token creation |
| `wallet_restore_from_seed` | Recovery (approval-gated) | `pko:StepExecution` / `bibo:Report` | NUT-09 / NUT-13 deterministic restore across allowlisted mints after native confirmation |
