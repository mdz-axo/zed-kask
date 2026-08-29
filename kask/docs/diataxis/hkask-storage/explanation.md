---
title: "hkask-storage — Explanation: Why Database Splits from SqliteDriver"
audience: [architects, developers]
last_updated: 2026-08-28
version: "2.0.0"
status: "Active"
domain: "Persistence"
mds_categories: [trust, curation]
---

# hkask-storage — Explanation: Why Database Splits from SqliteDriver

The consolidated `hkask-storage` crate separates two concerns: `Database`
handles file infrastructure (the SQLCipher salt file, parent directories,
passphrase validation), and `SqliteDriver` handles everything SQLite-related
(the r2d2 pool, PRAGMAs, schema initialization, query dispatch). This split
is not aesthetic — it eliminates dual-path bugs, and it lets stores code
against a provider-agnostic port (`DatabaseDriver`) instead of
`rusqlite::Connection` directly[^fowler-poeaa].

## Source citations

| Symbol | Location |
|--------|----------|
| `Database` struct (path + passphrase + pool_cache) | `kask/crates/hkask-storage/src/core/connection.rs:109-115` |
| `Database::open_impl` (file infra, no SQLite) | `kask/crates/hkask-storage/src/core/connection.rs:122-192` |
| `Database::sqlite_pool` (cached r2d2 pool) | `kask/crates/hkask-storage/src/core/connection.rs:261-283` |
| `file_pool` (passphrase probe before pool) | `kask/crates/hkask-storage/src/core/connection.rs:336-434` |
| `in_memory_pool` (max_size 1) | `kask/crates/hkask-storage/src/core/connection.rs:311-334` |
| `DatabaseError::PassphraseMismatch` / `SaltMissing` | `kask/crates/hkask-storage/src/core/connection.rs:89-97` |
| `open_or_repair` (self-heal on missing salt) | `kask/crates/hkask-storage/src/core/connection.rs:466-513` |
| `init_sqlite_vec_on` (per-connection, before schema) | `kask/crates/hkask-storage/src/core/connection.rs:56-76` |
| `DatabaseDriver` trait (the port) | `kask/crates/hkask-storage/src/database/driver.rs:16-58` |
| `SqliteDriver` (the only impl) | `kask/crates/hkask-storage/src/database/sqlite.rs:42-50` |
| `define_driver_store!` (store boilerplate) | `kask/crates/hkask-storage/src/core/store_macros.rs:44-71` |
| `HMemStore::from_driver` (no re-create of core table) | `kask/crates/hkask-storage/src/hmem.rs:140-163` |
| `RegulationArchive::init_schema` (store-specific table) | `kask/crates/hkask-storage/src/regulation_store.rs:76-104` |
| `Encryptor` (ENCv1 transparent encryption) | `kask/crates/hkask-storage/src/database/encrypt.rs:15-75` |

## The open/connect sequence

`open()` and `sqlite_pool()` are deliberately separate methods. `open()`
writes the salt file and creates parent directories; `sqlite_pool()` creates
the r2d2 pool, verifies the passphrase with a standalone probe connection,
loads sqlite-vec, sets PRAGMAs, and initializes the schema. One path for
file infrastructure, one path for SQLite — no dual-path bugs.

```mermaid
sequenceDiagram
    participant Caller
    participant Database
    participant FS as Filesystem
    participant Probe as Probe Connection
    participant Pool as r2d2 Pool
    participant Vec as sqlite-vec

    Caller->>Database: open(path, passphrase)
    Database->>FS: create_dir_all(parent)
    Database->>FS: read or write {path}.salt (16 bytes)
    Database-->>Caller: Database handle (no SQLite conn)

    Caller->>Database: sqlite_pool()
    Database->>FS: read salt
    Database->>Database: derive_key(passphrase, salt) via Argon2id
    Database->>Probe: open standalone connection
    Probe->>Probe: PRAGMA cipher_plaintext_header_size = 32
    Probe->>Probe: PRAGMA key = 'x"...""'
    Probe->>Probe: SELECT count(*) FROM sqlite_master
    alt passphrase wrong
        Probe-->>Database: error
        Database-->>Caller: Err(PassphraseMismatch)
        Note over Database,FS: files are NOT modified or deleted
    else passphrase correct
        Probe-->>Database: ok
        Database->>Pool: build r2d2 pool with with_init closure
        Pool->>Vec: init_sqlite_vec_on(conn) per connection
        Pool->>Pool: PRAGMA key + WAL_PRAGMA_BATCH + tuning
        Pool->>Pool: initialize_schema(schema.sql with $DIM)
        Pool-->>Database: pool
        Database-->>Caller: Ok(pool) (cached)
    end
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-STOR-005
verified_date: 2026-08-28
verified_against: kask/crates/hkask-storage/src/core/connection.rs:122-192,261-283,311-434,466-513; kask/crates/hkask-storage/src/database/sqlite.rs:24-35
status: VERIFIED
-->

## Why the split exists

### 1. No dual-path passphrase handling

A wrong passphrase can leave SQLCipher's native codec in a corrupted state;
when the pool later drops that connection during teardown, the codec
cleanup can SIGSEGV. `file_pool` therefore verifies the passphrase with a
**standalone probe connection** before creating the pool
(`core/connection.rs:352-365`): a wrong key fails the probe, the pool is
never built, and the codec cleanup runs on the probe alone. The pool only
ever holds connections with a validated key.

`open_or_repair` (`core/connection.rs:466-513`) enforces the P1 invariant
"a passphrase mistake never destroys my encrypted database" with one
explicit exception. A wrong passphrase returns `PassphraseMismatch` without
touching the database or salt file — pinned by the test
`open_or_repair_wrong_passphrase_does_not_delete_db`
(`core/connection.rs:602-640`). The exception: when the DB file exists but
its salt file is missing (`SaltMissing`, `core/connection.rs:159-170`), the
DB is permanently unopenable — no passphrase can decrypt it without its
original salt — so `open_or_repair` deletes the orphaned DB and creates a
fresh one. This is the "repair" the function name promises; a wrong
passphrase does NOT trigger it.

### 2. `Database` is the connection manager; `SqliteDriver` is the store handle

Stores hold `Arc<dyn DatabaseDriver>`, not `Arc<Database>`. `Database` is a
connection manager with a `Mutex<Option<Pool>>` cache
(`core/connection.rs:113-114`); `SqliteDriver` is the thin, cloneable,
`Send + Sync` handle that the `DatabaseDriver` trait requires. This lets
stores be passed across threads (the regulation loop, the curator ingest
path) without dragging the pool cache's lock along.

### 3. Stores code against a port, not `rusqlite`

The `DatabaseDriver` trait (`database/driver.rs:16-58`) is dyn-compatible:
`execute`, `execute_batch`, `query`, `query_optional`, `commit_tx`,
`rollback_tx`, `as_any`, `sqlite_pool`. Stores call these methods plus the
free-function helpers `query_map` / `query_row` (`database/driver.rs:78-109`)
and never touch `rusqlite::Connection` directly. The one exception is
`EmbeddingStore`, which needs the raw connection for `vec0` MATCH — it
downcasts via `sqlite_pool()` to acquire a connection
(`embeddings.rs:104-108`).

## Why per-store `init_schema` instead of a migration runner

The crate has no centralized migration runner. Each store owns its schema
and runs `CREATE TABLE IF NOT EXISTS` during `from_driver` construction.
The `define_driver_store!` macro generates `from_driver` to call
`Self::init_schema(driver)` and propagate any failure, so a store is never
constructed against a missing table (`core/store_macros.rs:44-71`).

Two ownership patterns coexist:

- **Core tables** (`hmems`, `embeddings`, `vec_embeddings`, `audit_log`,
  `memory_links`, `pod_meta`, `agent_registry`, `loop_cursors`,
  `reg_variety_checkpoint`, `reg_alerts`) live in `core/sql/schema.sql` and
  are loaded by `Database::initialize_schema` on every pool creation
  (`core/connection.rs:223-229`). Stores for these tables do not re-create
  them: `HMemStore::from_driver` explicitly does NOT re-create `hmems`
  (`hmem.rs:143-149`) — the prior `CREATE TABLE IF NOT EXISTS` here declared
  `recalled_at TEXT` nullable while `schema.sql` declared it
  `NOT NULL DEFAULT`, and the `IF NOT EXISTS` no-op meant the live schema
  depended on which ran first.
- **Store-specific tables** (`reg_records`, `reg_cursors`, `escalations`,
  the gallery tables) are created inline in the store's `init_schema`
  (`regulation_store.rs:76-104`, `escalation.rs:83-103`,
  `gallery.rs:193-270`).

The one schema migration that exists is column-level:
`migrate_embeddings_passage_text` (`core/connection.rs:236-250`) adds the
`passage_text` column to pre-existing `embeddings` tables via
`ALTER TABLE`, because `CREATE TABLE IF NOT EXISTS` cannot add columns to
an already-existing table.

```mermaid
stateDiagram-v2
    [*] --> DecideOwnership: new table needed
    DecideOwnership --> CoreSchema: shared across stores
    DecideOwnership --> StoreSchema: specific to one store
    CoreSchema --> LoadedByDatabase: schema.sql + initialize_schema
    StoreSchema --> LoadedByStore: init_schema in store module
    LoadedByDatabase --> NoOpInit: store init_schema returns Ok(())
    LoadedByStore --> InlineInit: store init_schema runs CREATE TABLE IF NOT EXISTS
    NoOpInit --> [*]: schema idempotent on every pool creation
    InlineInit --> [*]: schema idempotent on every from_driver
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-STOR-006
verified_date: 2026-08-28
verified_against: kask/crates/hkask-storage/src/core/connection.rs:223-250; kask/crates/hkask-storage/src/core/store_macros.rs:44-71; kask/crates/hkask-storage/src/hmem.rs:140-163; kask/crates/hkask-storage/src/regulation_store.rs:76-104; kask/crates/hkask-storage/src/escalation.rs:83-103
status: VERIFIED
-->

## Why the in-memory pool is `max_size(1)`

`SqliteConnectionManager::memory()` creates a separate in-memory database
per connection. A pool size greater than 1 would scatter writes across
independent databases, breaking read-your-writes semantics for tests
(`core/connection.rs:311-334`). The file pool, by contrast, defaults to
`max_size(8)` (overridable via `HKASK_DB_POOL_SIZE`, with a `warn!` on
malformed values, `core/connection.rs:395-409`).

## Why sqlite-vec is loaded per-connection

`init_sqlite_vec_on` (`core/connection.rs:56-76`) loads the `vec0`
extension into each connection via the r2d2 `with_init` closure, before
schema init (which creates `vec0` virtual tables). This avoids
`sqlite3_auto_extension`, whose process-global registration is deprecated
on Apple platforms and is a known teardown-segfault source. Scoping the
extension's lifetime to each connection means its state is torn down with
the connection, not orphaned at process exit.

## Why `EmbeddingStore` duplicates the vector BLOB

The vector BLOB is stored in both the `embeddings` table (metadata + vector)
and the `vec_embeddings` virtual table (KNN index). `vec0` requires the
vector for its MATCH operator; `embeddings.vector` provides uniform
retrieval via the backend-agnostic `DatabaseDriver` query path (`get`,
`get_all_by_prefix`). Deduplicating would require backend-conditional
retrieval — more complexity for ~4 KB/embedding savings. The redundancy
earns its keep by preserving the uniform retrieval abstraction
(`embeddings.rs:1-14`).

## Why `HMemStore` has an optional `Encryptor` — and why it is not yet wired

`HMemStore` holds an `encryptor: Option<Arc<Encryptor>>` field
(`hmem.rs:137`), and the `Encryptor` (`database/encrypt.rs:15-75`) does
transparent AES-256-GCM encryption of text values with an `ENCv1:` prefix
for automatic detection — plaintext passes through on decrypt, so a store
could migrate from unencrypted to encrypted without a schema change.
However, `HMemStore::from_driver` currently always sets `encryptor: None`
(`hmem.rs:155`), and no other constructor sets it: **value-level
encryption is not yet wired** — the encrypt/decrypt branches exist but are
unreachable in production. SQLCipher file-level encryption is the enforced
confidentiality mechanism today.

## See also

- [hkask-storage Reference](./reference.md): ERD of the full schema and the
  `DatabaseDriver` class diagram.
- [hkask-storage How-to](./how-to.md): procedural flowchart for adding a new
  store.
- [hkask-storage Tutorial](./tutorial.md): the store lifecycle from
  `Database` to CRUD.
- [`kask/docs/architecture/core/magna-carta.md`](../../architecture/core/magna-carta.md):
  P1 (User Sovereignty) governing `open_or_repair`'s passphrase-safety
  contract.

---

[^fowler-poeaa]: Fowler, M. (2002). *Patterns of Enterprise Application Architecture.* Addison-Wesley. <https://martinfowler.com/books/eaa.html>. The Repository pattern that motivates coding stores against a port trait instead of a concrete connection.

[^sqlcipher]: Zetetic LLC. (2024). *SQLCipher — Transparent SQLite Encryption.* <https://www.zetetic.net/sqlcipher/>. The encrypted SQLite extension whose codec-state corruption motivated the standalone passphrase probe.
