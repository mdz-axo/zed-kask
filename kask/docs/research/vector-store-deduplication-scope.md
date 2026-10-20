---
title: "Vector-store deduplication: scope and decision record"
audience: [architects, developers, operators]
last_updated: 2026-10-19
version: "1.1.0"
status: "Active"
domain: "Persistence"
mds_categories: [domain, lifecycle, trust]
---

# Vector-store deduplication: scope and decision record

Status: **scoped, not implemented** — awaiting an operator decision (see §6).
Composed: 2026-10-18. Evidence commit: `31d19d877b` (the distribution-side reduction that ships today).

## 1. The question

`embeddings.vector` and vec0's `vec_embeddings.embedding` hold the same 1024-float
vector. On the zk-ref-open fine index that is **1,105,186,816 bytes (1.03 GiB)**
of duplicated data — the largest single remaining reduction in the corpus
artifact and in every per-agent store.

Can one copy be removed without changing what any reader retrieves?

## 2. What was measured (not assumed)

**The documented "escape hatch" does not work.** The module comment in
`hkask-storage/src/embeddings.rs` claimed a vec0 auxiliary column
(`+vector BLOB`) would eliminate the copy. Reading the sqlite-vec 0.1.9 C
source falsifies this:

- `vec0_parse_auxiliary_column_definition` (`sqlite-vec.c:2075`) accepts a `+`
  prefix and the types text/integer/float/double/blob.
- The column is materialized into its **own shadow table**:
  `VEC0_SHADOW_AUXILIARY_NAME = "%w"."%w_auxiliary"`, created as
  `CREATE TABLE ..._auxiliary(rowid integer PRIMARY KEY, value00 …)`
  (`sqlite-vec.c:5225`), written by a separate
  `INSERT INTO ..._auxiliary(rowid, value00 …) VALUES (?, ?)` (`:8490`).

An auxiliary column is therefore a **second copy in a new table**, not a
reference to the vector vec0 already stores. Adding it would *grow* the
database by ~1.03 GiB. The comment has been corrected in `31d19d877b`.

**The real dedupe is available.** vec0 returns its `embedding` column
byte-for-byte identical to the canonical blob. Pinned by
`hkask-storage/tests/vector_dedupe_probe.rs::vec0_returns_the_canonical_vector_byte_for_byte`
(passes). So `embeddings.vector` can be dropped and the vector read back from
vec0.

**The strip path works on a bare connection.** The same probe file pins that
`DROP TABLE` + `VACUUM` run through a plain keyed rusqlite connection with no
vec0 module loaded, on both a standalone database and an ATTACHed one, and
preserve every vec0 shadow row.

## 3. The change boundary

Removing the column touches the **canonical store for every database**,
including the read-write curator store — not just the corpus path.

### Writers
- `store` (`embeddings.rs:227`) — `INSERT INTO embeddings (... vector ...)`
- `replace` (`:283`) — same insert
- `rebuild_vector_index` (`rotation.rs:648`) — currently rebuilds vec0 **from**
  `embeddings.vector`; after the change this direction inverts or disappears

### Readers (all currently `SELECT ... vector FROM embeddings`)
- `get` (`:329`), `get_by_ids` (`:499`), `get_all_by_prefix` (`:740`),
  `all_with_text` (`:832`)
- `search` (`:401`) — already joins vec0, but selects `e.vector` for the result

### Direct-SQL consumers outside the store (must be re-pointed or given an API)
- `hkask-mcp-corpus/src/services/prompt_builder.rs:356` —
  `SELECT entity_ref, vector, passage_text FROM embeddings`
- `hkask-mcp-corpus/src/index.rs` — vector-length validation
- `hkask-mcp-corpus/src/retrieval_tests.rs` (`RecordingPort::generate`) — a test that `UPDATE`s the
  column (must be re-expressed against vec0)

### The admission contract
`hkask-memory/src/federated_recall.rs:31` — `CURRENT_EMBEDDING_COLUMNS`
includes `"vector"`. A shipped source missing it **fails admission** with
`SchemaMismatch`. The constant and `validate_current_schema` must change in the
same pass, and the change is a **format break**: a new bundle cannot be read by
an older binary, and an old bundle cannot be read by a new one unless the
validator accepts both shapes during a transition.

## 4. Options

**A. Keep both copies (today).** No code change. 1.03 GiB duplicated per store.
Correctness: proven.

**B. Drop `embeddings.vector`; read the vector from vec0.** One physical copy.
- Requires the reader paths in §3 to join vec0, and `store` to write only
  `vec_embeddings`.
- Cost: the uniform `DatabaseDriver` retrieval abstraction is lost for vectors —
  reads become backend-conditional (the concern the original comment named).
- Risk: every existing database (curator, corpus, styles, per-agent) needs a
  migration that drops the column and verifies vec0 is complete first; a
  database whose vec0 is short of rows would lose vectors. `delete_orphaned_vectors`
  is the inverse check and must run before, not after.
- Format break: yes (admission column contract).

**C. Keep `embeddings.vector` canonical; stop maintaining `vec0` for stores that
do not need KNN.** Reclaims the vec0 shadow tables (1.15 GiB measured on the
fine index) instead of the blob, for stores whose only access is
`get`/`get_by_ids`/prefix scans. No format break for the KNN path — but a
database opened by a path that expects KNN would fail, so it needs a per-store
capability flag, and the corpus index genuinely needs KNN.

**D. Distribution-side only (already shipped).** `strip_producer_tables`
removes tables no read path uses (`hmems`, −25.6%). Orthogonal to this decision
and already committed.

## 5. Recommendation

**B, as its own change, with a two-phase migration** — and not as a bolt-on:

1. **Phase 1 (additive, no format break):** add a store method that reads
   vectors from vec0, and a migration that *verifies* vec0 completeness per
   database (`vec0 rows == embeddings rows`, no orphans) without dropping
   anything. Ship the verification; if any database fails it, stop and report.
2. **Phase 2 (the drop):** drop `embeddings.vector`, update
   `CURRENT_EMBEDDING_COLUMNS`, re-point the direct-SQL consumers, and re-seal
   the corpus. Requires a bundle format version bump so old and new binaries
   each reject the other's shape with a clear message rather than a schema
   mismatch.

Why not now: it changes the canonical store for the **read-write curator
database**, where a bug is not recoverable from a sealed artifact. The
distribution reduction already committed captures the safe majority of the
size win, and the remaining 1.03 GiB is best taken with its own design,
migration, and evaluation rather than inside a corpus recompose.

## 6. Decision requested

- **Take B** (two-phase, as above) as a separate work item — recommended.
- **Take C** for stores that do not need KNN, leaving the corpus index on B's
  path later.
- **Decline** the dedupe: keep both copies; the size is acceptable.

Whichever is chosen, the probe file
(`hkask-storage/tests/vector_dedupe_probe.rs`) already pins the two facts the
work depends on: vec0 returns the canonical bytes, and strip+vacuum work
without the vec0 module loaded.

## 7. What is *not* in scope here

- The `reqwest` removal and the `hmems` strip — committed in `31d19d877b`.
- The orphan vec0 rows in the **authoring** databases (13,126 rows / ~56 MB in
  `fine.db`, 7,401 in `current.db`, 4,869 in `reference.db`). These are
  interior rowids left by re-embedding churn; KNN's inner join already ignores
  them, and the export path purges them from the shipped copy
  (`rebuild_vector_index`). They remain in the authoring DBs. `EmbeddingStore::
  delete_orphaned_vectors` is the ready-made sweep; running it against the
  authoring DBs is a one-line maintenance action, deliberately left to the
  operator rather than performed here because those databases are live inputs
  to other work.
