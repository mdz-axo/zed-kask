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

| Variable | Description |
|----------|-------------|
| `HKASK_DB_PROVIDER` | Database provider (`sqlite` or `postgres`) |
| `HKASK_DB_PATH` | SQLite database path |
| `HKASK_DB_PASSPHRASE` | Database encryption passphrase |

## Schema