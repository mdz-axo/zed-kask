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

This is the storage primitive, not live maintenance orchestration. Coordinated
settings/curator quiescence and recovery remain open in core-review T11;
individual file renames do not establish multi-database/keychain crash atomicity.

## Schema
