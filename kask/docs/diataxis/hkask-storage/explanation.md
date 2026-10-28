---
title: "hkask-storage — Explanation: Boundaries and Maintenance"
audience: [architects, developers]
last_updated: 2026-10-13
version: "2.3.0"
status: "Active"
domain: "Persistence"
mds_categories: [trust, curation]
---

# hkask-storage — Explanation: Boundaries and Maintenance

`hkask-storage` separates connection infrastructure, a database driver port, and
domain stores. `Database` owns path preparation, passphrase validation, SQLCipher
pool creation, maintenance leases, and core-schema initialization
(`kask/crates/hkask-storage/src/core/connection.rs:166-192,209-345,420-452`).
`SqliteDriver` is the cloneable implementation of the provider-neutral
`DatabaseDriver` port
(`kask/crates/hkask-storage/src/database/driver.rs:15-47`;
`kask/crates/hkask-storage/src/database/sqlite.rs:42-117`). This is the Repository
and Data Mapper separation: domain stores depend on a persistence contract rather
than embedding connection management.[^fowler-poeaa]

## Why database encryption is a connection concern

File-backed `Database` pools use SQLCipher page encryption. The passphrase is
applied through `PRAGMA key`; SQLCipher derives the page key and stores its salt in
the database header
(`kask/crates/hkask-storage/src/core/connection.rs:514-536,590`). `DbValue` is the typed SQL binding representation
(`kask/crates/hkask-storage/src/database/value.rs:7-46`); encryption is supplied
by the SQLCipher-backed connection rather than by field wrappers.[^sqlcipher]

A standalone probe verifies the passphrase before the r2d2 pool is built, so the
pool never retains a connection initialized with a wrong key
(`kask/crates/hkask-storage/src/core/connection.rs:522-536`). In-memory pools are
unencrypted and use one connection because separate SQLite in-memory connections
would be separate databases
(`kask/crates/hkask-storage/src/core/connection.rs:474-483`).

```mermaid
sequenceDiagram
    participant Caller
    participant DB as Database
    participant Inventory as Managed inventory
    participant Lease as Maintenance lease
    participant Probe as SQLCipher probe
    participant Pool as r2d2 pool
    Caller->>DB: open(path, passphrase)
    DB-->>Caller: validated handle
    Caller->>DB: sqlite_pool()
    DB->>Lease: acquire shared database lease
    DB->>Inventory: record canonical managed path
    DB->>Probe: PRAGMA key + sqlite_master query
    alt key rejected
        Probe-->>DB: PassphraseMismatch
        DB-->>Caller: error; source retained
    else key accepted
        DB->>Pool: create initialized leased pool
        Pool-->>Caller: cached pool
    end
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-STOR-005
verified_date: 2026-09-28
verified_against: kask/crates/hkask-storage/src/core/connection.rs:209-345,420-452,474-483,514-536,582; kask/crates/hkask-storage/src/maintenance_inventory.rs:41-48,103-152
status: VERIFIED
-->

## Why maintenance inventory is an attestation, not a lock

Passphrase rotation needs a complete path set before any key changes. The
`maintenance_inventory` module records managed database paths in a locked JSONL
catalog and can combine configured paths, explicit external paths, and discovered
maintenance markers into a bounded preview
(`kask/crates/hkask-storage/src/maintenance_inventory.rs:1-20,41-152,206-293`).
Missing, corrupt, oversized, or incomplete catalog data is an error rather than an
empty inventory (`kask/crates/hkask-storage/src/maintenance_inventory.rs:57-101`).

`DatabaseInventory::confirm` requires a fresh identical preview, explicit
attestation that historical/external scope is complete, reasons for exclusions,
no unresolved recovery artifacts, and unambiguous file identity
(`kask/crates/hkask-storage/src/maintenance_inventory.rs:294-375`). The resulting
`ConfirmedInventory` is only a receipt for the selected path set; it is not a
quiescence grant and does not rotate or publish a passphrase
(`kask/crates/hkask-storage/src/maintenance_inventory.rs:186-205`).

This separation avoids a dangerous category error: knowing which files exist does
not prove that every process has released them.

## Why schemas remain store-owned

Core tables are initialized from `core/sql/schema.sql`; column-level migrations
run immediately afterward
(`kask/crates/hkask-storage/src/core/connection.rs:347-392`). Domain-specific tables
are created by their stores, including the Regulation tables
(`kask/crates/hkask-storage/src/regulation_store.rs:38-70`). The gallery store's tables
moved with it to the media server (2026-10-13, commit `c85b1e4896`) — see
[Media MCP Server Reference](../../reference/mcp-servers/media.md). The
`define_driver_store!` macro makes construction fail if a store's schema
initialization fails (`kask/crates/hkask-storage/src/core/store_macros.rs:44-71`).

## See also

- [hkask-storage reference — API surface and procedures](./reference.md)
- [Standardized artifact storage](../../architecture/standardized-artifact-storage.md)

---

[^fowler-poeaa]: Fowler, M. (2002). *Patterns of Enterprise Application Architecture.* Addison-Wesley. <https://martinfowler.com/books/eaa.html>.
[^sqlcipher]: Zetetic LLC. (2024). *SQLCipher — Transparent SQLite Encryption.* <https://www.zetetic.net/sqlcipher/>.