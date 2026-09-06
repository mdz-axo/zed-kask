# hkask-storage

SQLite + SQLCipher storage for hKask.

## Features

- **SQLCipher** — encrypted at rest via passphrase-derived key
- **sqlite-vec** — vector embeddings for semantic search
- **BLAKE3** — content-addressable storage
- **gix** — Git-based CAS (Content-Addressable Storage) port
- **Triples** — subject-predicate-object store with WebID ownership
- **Blobs** — binary large object storage
- **Backup** — snapshot, restore, prune, verify

## Configuration

| Variable              | Description                    |
| --------------------- | ------------------------------ |
| `HKASK_DB_PATH`       | SQLite database path           |
| `HKASK_DB_PASSPHRASE` | Database encryption passphrase |

## Passphrase rotation

`rotate_passphrase` requires all other database consumers to be closed first.
It exports the schema/data into an encrypted copy with SQLCipher, preserves RSS
FTS indexes/triggers and row identities, rebuilds KNN from canonical embedding
rows, and checks foreign keys and integrity before replacing the source.
Invalid vectors or failed preservation checks abort without publishing the copy.

File-backed SQLCipher pools use `SqliteConnectionManager` and
`LeasedSqliteConnection`: each connection retains a shared OS file lease until
its SQLite handle closes. Rotation requires an exclusive lease held through
replacement. Pool clones, checked-out connections, and background pool workers
cannot release that protection prematurely. New pool opens fail during exclusive
maintenance. The stable `<canonical-db>.maintenance-lock` file is empty/non-secret;
never unlink it to bypass a lock. Rotation follows symlinks to their canonical
source and rejects hard-link aliases, which do not share a per-path lease.

Existing `.new`/`.old` recovery files or their sidecars cause `RecoveryRequired`
before rotation touches the source. Ordinary opens also refuse these artifacts,
including when an interrupted rename left only a backup: startup must not create
an empty replacement. Reconcile recovery explicitly; retry is not cleanup.

This is the storage primitive, not live maintenance orchestration. The operator
approved maintenance restart on 2026-09-06, but inventory confirmation, operation
drain, helper handoff, multi-file journal/key publication, and visible reopen/resume
remain open in core-review T11. These leases cover participating storage users,
not older binaries or direct SQLite opens. Individual file renames do not establish
multi-database/keychain crash atomicity.

## Schema
